use crate::error::ArchiveError;
use crate::limits::MAX_ARCHIVE_WORK;
use crate::limits::MAX_XML_ATTRIBUTES_PER_ELEMENT;
use crate::limits::MAX_XML_BYTES;
use crate::limits::MAX_XML_DEPTH;
use crate::limits::MAX_XML_NODES;
use crate::limits::MAX_XML_TEXT_BYTES;
use domain::case_fold_key;
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::unescape;
use quick_xml::events::BytesStart;
use quick_xml::events::Event;
use quick_xml::name::NamespaceResolver;
use quick_xml::name::ResolveResult;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::BTreeSet;
use std::str::from_utf8;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FomodDocument {
	pub(crate) schema_version: String,
	pub(crate) root: NormalizedElement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NormalizedElement {
	pub(crate) name: String,
	pub(crate) namespace: Option<String>,
	pub(crate) attributes: Vec<NormalizedAttribute>,
	pub(crate) text: String,
	pub(crate) children: Vec<NormalizedElement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NormalizedAttribute {
	pub(crate) name: String,
	pub(crate) namespace: Option<String>,
	pub(crate) value: String,
}

pub(crate) fn parse(bytes: &[u8], cancellation: &CancellationToken) -> Result<FomodDocument, ArchiveError> {
	let root = parse_tree(bytes, cancellation)?;
	if !root.name.eq_ignore_ascii_case("config") {
		return Err(report!(ArchiveError::UnsupportedInstaller));
	}
	let schema_version = detect_schema_version(&root)?;
	validate_semantics(&root)?;
	Ok(FomodDocument { schema_version, root })
}

// quick-xml supplies secure event parsing, but FOMM needs a bounded normalized tree for its own typed grammar.
fn parse_tree(bytes: &[u8], cancellation: &CancellationToken) -> Result<NormalizedElement, ArchiveError> {
	let started = Instant::now();
	if bytes.len() as u64 > MAX_XML_BYTES {
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	let (mut xml, source_encoding) = decode_xml(bytes)?;
	let declared_encoding = declared_encoding(&xml)?;
	validate_declared_encoding(source_encoding, declared_encoding.as_deref())?;
	normalize_declaration(&mut xml)?;

	let mut reader = NsReader::from_str(&xml);
	reader.config_mut().trim_text(false);
	reader.config_mut().check_end_names = true;
	let mut stack: Vec<NormalizedElement> = Vec::new();
	let mut root = None;
	let mut node_count = 0_usize;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let event = reader.read_event().context(ArchiveError::InvalidXml)?;
		match event {
			Event::Start(start) => {
				node_count = node_count
					.checked_add(1)
					.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
				if node_count > MAX_XML_NODES || stack.len() >= MAX_XML_DEPTH {
					return Err(report!(ArchiveError::ExpansionLimit));
				}
				let (namespace, _) = reader.resolver().resolve_element(start.name());
				let element = decode_element(namespace, &start, reader.resolver())?;
				validate_element_security(&element)?;
				stack.push(element);
			}
			Event::Empty(start) => {
				node_count = node_count
					.checked_add(1)
					.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
				if node_count > MAX_XML_NODES || stack.len() >= MAX_XML_DEPTH {
					return Err(report!(ArchiveError::ExpansionLimit));
				}
				let (namespace, _) = reader.resolver().resolve_element(start.name());
				let element = decode_element(namespace, &start, reader.resolver())?;
				validate_element_security(&element)?;
				push_element(&mut stack, &mut root, element)?;
			}
			Event::End(_) => {
				let element = stack.pop().ok_or_else(|| report!(ArchiveError::InvalidXml))?;
				push_element(&mut stack, &mut root, element)?;
			}
			Event::Text(text) => {
				let decoded = text.xml_content(XmlVersion::Implicit1_0);
				let decoded = unescape(&decoded).context(ArchiveError::InvalidXml)?;
				push_text(&mut stack, &decoded)?;
			}
			Event::CData(text) => {
				let decoded = text.xml_content(XmlVersion::Implicit1_0);
				push_text(&mut stack, &decoded)?;
			}
			Event::GeneralRef(reference) => {
				let encoded = format!("&{};", reference.as_ref());
				let decoded = unescape(&encoded).context(ArchiveError::InvalidXml)?;
				push_text(&mut stack, &decoded)?;
			}
			Event::DocType(_) | Event::PI(_) => return Err(report!(ArchiveError::InvalidXml)),
			Event::Decl(_) | Event::Comment(_) => {}
			Event::Eof => break,
		}
	}
	if !stack.is_empty() {
		return Err(report!(ArchiveError::InvalidXml));
	}
	root.ok_or_else(|| report!(ArchiveError::InvalidXml))
}

fn decode_element(
	namespace: ResolveResult<'_>,
	start: &BytesStart<'_>,
	resolver: &NamespaceResolver,
) -> Result<NormalizedElement, ArchiveError> {
	let namespace = match namespace {
		ResolveResult::Unbound => None,
		ResolveResult::Bound(namespace) => Some(namespace.into_inner().to_owned()),
		ResolveResult::Unknown(_) => return Err(report!(ArchiveError::InvalidXml)),
	};
	let qualified_name = start.name().as_ref().to_owned();
	let name = local_name(&qualified_name).to_owned();
	let mut attributes = Vec::new();
	let mut seen = BTreeSet::new();
	for attribute in start.attributes().with_checks(true) {
		let attribute = attribute.context(ArchiveError::InvalidXml)?;
		let qualified_name = attribute.key.as_ref();
		if qualified_name == "xmlns" || qualified_name.starts_with("xmlns:") {
			continue;
		}
		let (namespace, local_name) = resolver.resolve_attribute(attribute.key);
		let namespace = match namespace {
			ResolveResult::Unbound => None,
			ResolveResult::Bound(namespace) => Some(namespace.into_inner().to_owned()),
			ResolveResult::Unknown(_) => return Err(report!(ArchiveError::InvalidXml)),
		};
		let name = local_name.as_ref().to_owned();
		if !seen.insert((namespace.clone(), case_fold_key(&name)))
			|| attributes.len() >= MAX_XML_ATTRIBUTES_PER_ELEMENT
		{
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		let value = attribute
			.normalized_value(XmlVersion::Implicit1_0)
			.context(ArchiveError::InvalidXml)?
			.into_owned();
		if value.len() > MAX_XML_TEXT_BYTES {
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		attributes.push(NormalizedAttribute { name, namespace, value });
	}
	Ok(NormalizedElement {
		name,
		namespace,
		attributes,
		text: String::new(),
		children: Vec::new(),
	})
}

fn push_element(
	stack: &mut [NormalizedElement],
	root: &mut Option<NormalizedElement>,
	element: NormalizedElement,
) -> Result<(), ArchiveError> {
	if let Some(parent) = stack.last_mut() {
		parent.children.push(element);
	} else if root.replace(element).is_some() {
		return Err(report!(ArchiveError::InvalidXml));
	}
	Ok(())
}

fn push_text(stack: &mut [NormalizedElement], text: &str) -> Result<(), ArchiveError> {
	if text.is_empty() || stack.is_empty() && text.chars().all(char::is_whitespace) {
		return Ok(());
	}
	let element = stack.last_mut().ok_or_else(|| report!(ArchiveError::InvalidXml))?;
	if element
		.text
		.len()
		.checked_add(text.len())
		.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?
		> MAX_XML_TEXT_BYTES
	{
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	element.text.push_str(text);
	Ok(())
}

fn validate_element_security(element: &NormalizedElement) -> Result<(), ArchiveError> {
	if element.namespace.is_some()
		|| element.name.eq_ignore_ascii_case("include")
		|| element.name.eq_ignore_ascii_case("fallback")
		|| element.attributes.iter().any(|attribute| {
			attribute.namespace.is_some()
				&& !(element.name.eq_ignore_ascii_case("config")
					&& attribute.namespace.as_deref() == Some(XSI_NAMESPACE)
					&& is_schema_location_attribute(&attribute.name))
		}) {
		return Err(report!(ArchiveError::InvalidXml));
	}
	Ok(())
}

fn is_schema_location_attribute(name: &str) -> bool {
	name.eq_ignore_ascii_case("noNamespaceSchemaLocation") || name.eq_ignore_ascii_case("schemaLocation")
}

fn detect_schema_version(root: &NormalizedElement) -> Result<String, ArchiveError> {
	let hint = root
		.attributes
		.iter()
		.find(|attribute| {
			(attribute.namespace.is_none() || attribute.namespace.as_deref() == Some(XSI_NAMESPACE))
				&& attribute.name.eq_ignore_ascii_case("noNamespaceSchemaLocation")
		})
		.map(|attribute| attribute.value.as_str())
		.or_else(|| {
			root.attributes
				.iter()
				.find(|attribute| {
					attribute.namespace.is_none() && attribute.name.eq_ignore_ascii_case("version")
				})
				.map(|attribute| attribute.value.as_str())
		})
		.ok_or_else(|| report!(ArchiveError::UnsupportedInstaller))?;
	let lowercase = hint.to_ascii_lowercase();
	let basename = lowercase.rsplit(['/', '\\']).next().unwrap_or(&lowercase);
	let version = if basename == "5.1" || basename.contains("modconfig5.1") || basename.contains("xmlscript5.1") {
		"5.1"
	} else if basename == "5.0"
		|| basename == "5.x"
		|| basename.contains("modconfig5.0")
		|| basename.contains("xmlscript5.0")
		|| basename.contains("xmlscript5.x")
	{
		"5.0"
	} else {
		return Err(report!(ArchiveError::UnsupportedInstaller));
	};
	if lowercase.contains(".xsd") && !basename.starts_with("modconfig5") && !basename.starts_with("xmlscript5") {
		return Err(report!(ArchiveError::UnsupportedInstaller));
	}
	Ok(version.to_owned())
}

fn validate_semantics(root: &NormalizedElement) -> Result<(), ArchiveError> {
	validate_children(
		root,
		&[
			"moduleName",
			"moduleImage",
			"moduleDependencies",
			"requiredInstallFiles",
			"installSteps",
			"conditionalFileInstalls",
		],
	)?;
	validate_child_count(root, "moduleName", 1, 1)?;
	for child in [
		"moduleImage",
		"moduleDependencies",
		"requiredInstallFiles",
		"installSteps",
		"conditionalFileInstalls",
	] {
		validate_child_count(root, child, 0, 1)?;
	}
	walk_semantics(root)
}

fn walk_semantics(element: &NormalizedElement) -> Result<(), ArchiveError> {
	let allowed_children: Option<&[&str]> = if matches_name(&element.name, &["files", "requiredInstallFiles"]) {
		Some(&["file", "folder"])
	} else if matches_name(&element.name, &["moduleDependencies", "dependencies", "visible"]) {
		Some(&[
			"fileDependency",
			"flagDependency",
			"gameDependency",
			"fommDependency",
			"nvseDependency",
			"dependencies",
		])
	} else if element.name.eq_ignore_ascii_case("config") {
		None
	} else if element.name.eq_ignore_ascii_case("installSteps") {
		validate_child_count(element, "installStep", 1, usize::MAX)?;
		Some(&["installStep"])
	} else if element.name.eq_ignore_ascii_case("installStep") {
		validate_child_count(element, "visible", 0, 1)?;
		validate_child_count(element, "optionalFileGroups", 1, 1)?;
		Some(&["visible", "optionalFileGroups"])
	} else if element.name.eq_ignore_ascii_case("optionalFileGroups") {
		validate_child_count(element, "group", 1, usize::MAX)?;
		Some(&["group"])
	} else if element.name.eq_ignore_ascii_case("group") {
		validate_child_count(element, "plugins", 1, 1)?;
		Some(&["plugins"])
	} else if element.name.eq_ignore_ascii_case("plugins") {
		validate_child_count(element, "plugin", 1, usize::MAX)?;
		Some(&["plugin"])
	} else if element.name.eq_ignore_ascii_case("plugin") {
		for child in ["description", "image", "files", "conditionFlags"] {
			validate_child_count(element, child, 0, 1)?;
		}
		validate_child_count(element, "typeDescriptor", 1, 1)?;
		Some(&["description", "image", "files", "conditionFlags", "typeDescriptor"])
	} else if element.name.eq_ignore_ascii_case("conditionFlags") {
		validate_child_count(element, "flag", 1, usize::MAX)?;
		Some(&["flag"])
	} else if element.name.eq_ignore_ascii_case("typeDescriptor") {
		if element.children.len() != 1 {
			return Err(report!(ArchiveError::UnsupportedInstaller));
		}
		Some(&["type", "dependencyType"])
	} else if element.name.eq_ignore_ascii_case("dependencyType") {
		validate_child_count(element, "defaultType", 1, 1)?;
		validate_child_count(element, "patterns", 1, 1)?;
		Some(&["defaultType", "patterns"])
	} else if element.name.eq_ignore_ascii_case("conditionalFileInstalls") {
		validate_child_count(element, "patterns", 1, 1)?;
		Some(&["patterns"])
	} else if element.name.eq_ignore_ascii_case("patterns") {
		validate_child_count(element, "pattern", 1, usize::MAX)?;
		Some(&["pattern"])
	} else if element.name.eq_ignore_ascii_case("pattern") {
		validate_child_count(element, "dependencies", 1, 1)?;
		let type_count = child_count(element, "type");
		let files_count = child_count(element, "files");
		if (type_count, files_count) != (1, 0) && (type_count, files_count) != (0, 1) {
			return Err(report!(ArchiveError::UnsupportedInstaller));
		}
		Some(&["dependencies", "type", "files"])
	} else if matches_name(
		&element.name,
		&[
			"file",
			"folder",
			"fileDependency",
			"flagDependency",
			"gameDependency",
			"fommDependency",
			"nvseDependency",
			"type",
			"defaultType",
			"flag",
			"description",
			"image",
			"moduleName",
			"moduleImage",
		],
	) {
		Some(&[])
	} else {
		None
	};
	if let Some(allowed) = allowed_children {
		validate_children(element, allowed)?;
	}
	validate_attributes(element)?;
	for child in &element.children {
		walk_semantics(child)?;
	}
	Ok(())
}

fn validate_attributes(element: &NormalizedElement) -> Result<(), ArchiveError> {
	let allowed: &[&str] = if element.name.eq_ignore_ascii_case("config") {
		&["noNamespaceSchemaLocation", "schemaLocation", "version"]
	} else if element.name.eq_ignore_ascii_case("moduleName") {
		&["position", "colour", "color"]
	} else if element.name.eq_ignore_ascii_case("moduleImage") {
		&["path", "showImage", "showFade", "height"]
	} else if element.name.eq_ignore_ascii_case("image") {
		&["path"]
	} else if matches_name(&element.name, &["moduleDependencies", "dependencies", "visible"]) {
		&["operator"]
	} else if element.name.eq_ignore_ascii_case("fileDependency") {
		&["file", "state"]
	} else if element.name.eq_ignore_ascii_case("flagDependency") {
		&["flag", "value"]
	} else if matches_name(&element.name, &["gameDependency", "fommDependency", "nvseDependency"]) {
		&["version"]
	} else if matches_name(&element.name, &["installSteps", "optionalFileGroups", "plugins"]) {
		&["order"]
	} else if element.name.eq_ignore_ascii_case("installStep") {
		&["name"]
	} else if element.name.eq_ignore_ascii_case("group") {
		&["name", "type"]
	} else if matches_name(&element.name, &["plugin", "type", "defaultType", "flag"]) {
		&["name"]
	} else if matches_name(&element.name, &["file", "folder"]) {
		&["source", "destination", "alwaysInstall", "installIfUsable", "priority"]
	} else {
		&[]
	};
	if element
		.attributes
		.iter()
		.any(|attribute| !allowed.iter().any(|name| attribute.name.eq_ignore_ascii_case(name)))
	{
		return Err(report!(ArchiveError::UnsupportedInstaller));
	}
	Ok(())
}

fn child_count(element: &NormalizedElement, name: &str) -> usize {
	element.children
		.iter()
		.filter(|child| child.name.eq_ignore_ascii_case(name))
		.count()
}

fn validate_child_count(
	element: &NormalizedElement,
	name: &str,
	minimum: usize,
	maximum: usize,
) -> Result<(), ArchiveError> {
	let count = child_count(element, name);
	if count < minimum || count > maximum {
		return Err(report!(ArchiveError::UnsupportedInstaller));
	}
	Ok(())
}

fn validate_children(element: &NormalizedElement, allowed: &[&str]) -> Result<(), ArchiveError> {
	if element
		.children
		.iter()
		.any(|child| !allowed.iter().any(|name| child.name.eq_ignore_ascii_case(name)))
	{
		return Err(report!(ArchiveError::UnsupportedInstaller));
	}
	Ok(())
}

fn matches_name(name: &str, expected: &[&str]) -> bool {
	expected.iter().any(|candidate| name.eq_ignore_ascii_case(candidate))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceEncoding {
	Utf8,
	Utf16Le,
	Utf16Be,
}

fn decode_xml(bytes: &[u8]) -> Result<(String, SourceEncoding), ArchiveError> {
	if let Some(body) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
		return Ok((
			from_utf8(body).context(ArchiveError::InvalidXml)?.to_owned(),
			SourceEncoding::Utf8,
		));
	}
	if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
		return decode_utf16(body, SourceEncoding::Utf16Le);
	}
	if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
		return decode_utf16(body, SourceEncoding::Utf16Be);
	}
	Ok((
		from_utf8(bytes).context(ArchiveError::InvalidXml)?.to_owned(),
		SourceEncoding::Utf8,
	))
}

// quick-xml's encoding feature does not support UTF-16, which occurs in FOMOD configurations.
fn decode_utf16(bytes: &[u8], encoding: SourceEncoding) -> Result<(String, SourceEncoding), ArchiveError> {
	if !bytes.len().is_multiple_of(2) {
		return Err(report!(ArchiveError::InvalidXml));
	}
	let words = bytes
		.as_chunks::<2>()
		.0
		.iter()
		.map(|bytes| match encoding {
			SourceEncoding::Utf16Le => u16::from_le_bytes([bytes[0], bytes[1]]),
			SourceEncoding::Utf16Be => u16::from_be_bytes([bytes[0], bytes[1]]),
			SourceEncoding::Utf8 => unreachable!(),
		})
		.collect::<Vec<_>>();
	Ok((String::from_utf16(&words).context(ArchiveError::InvalidXml)?, encoding))
}

fn declared_encoding(xml: &str) -> Result<Option<String>, ArchiveError> {
	let prefix = xml
		.get(..xml.len().min(512))
		.ok_or_else(|| report!(ArchiveError::InvalidXml))?;
	if !prefix.trim_start().starts_with("<?xml") {
		return Ok(None);
	}
	let end = prefix.find("?>").ok_or_else(|| report!(ArchiveError::InvalidXml))?;
	let declaration = &prefix[..end];
	let lowercase = declaration.to_ascii_lowercase();
	let Some(position) = lowercase.find("encoding") else {
		return Ok(None);
	};
	let after = declaration[position + "encoding".len()..].trim_start();
	let after = after
		.strip_prefix('=')
		.ok_or_else(|| report!(ArchiveError::InvalidXml))?
		.trim_start();
	let quote = after.chars().next().ok_or_else(|| report!(ArchiveError::InvalidXml))?;
	if quote != '\'' && quote != '"' {
		return Err(report!(ArchiveError::InvalidXml));
	}
	let value = after[quote.len_utf8()..]
		.split(quote)
		.next()
		.ok_or_else(|| report!(ArchiveError::InvalidXml))?;
	Ok(Some(value.to_owned()))
}

fn validate_declared_encoding(source: SourceEncoding, declared: Option<&str>) -> Result<(), ArchiveError> {
	let Some(declared) = declared else {
		return Ok(());
	};
	let declared = declared.to_ascii_lowercase().replace('_', "-");
	let matches = match source {
		SourceEncoding::Utf8 => matches!(declared.as_str(), "utf-8" | "us-ascii"),
		SourceEncoding::Utf16Le => matches!(declared.as_str(), "utf-16" | "utf-16le"),
		SourceEncoding::Utf16Be => matches!(declared.as_str(), "utf-16" | "utf-16be"),
	};
	matches.then_some(()).ok_or_else(|| report!(ArchiveError::InvalidXml))
}

fn normalize_declaration(xml: &mut String) -> Result<(), ArchiveError> {
	let Some(end) = xml.get(..xml.len().min(512)).and_then(|prefix| prefix.find("?>")) else {
		return Ok(());
	};
	if !xml[..end].trim_start().starts_with("<?xml") {
		return Ok(());
	}
	let declaration = &xml[..end];
	let lowercase = declaration.to_ascii_lowercase();
	let Some(position) = lowercase.find("encoding") else {
		return Ok(());
	};
	let value_start_relative = declaration[position + "encoding".len()..]
		.find(['\'', '"'])
		.ok_or_else(|| report!(ArchiveError::InvalidXml))?;
	let quote_position = position + "encoding".len() + value_start_relative;
	let quote = xml.as_bytes()[quote_position] as char;
	let value_start = quote_position + 1;
	let value_end = xml[value_start..end]
		.find(quote)
		.map(|offset| value_start + offset)
		.ok_or_else(|| report!(ArchiveError::InvalidXml))?;
	xml.replace_range(value_start..value_end, "UTF-8");
	Ok(())
}

fn local_name(qualified_name: &str) -> &str {
	qualified_name
		.rsplit_once(':')
		.map_or(qualified_name, |(_, local)| local)
}

#[cfg(test)]
mod tests {
	use super::parse;
	use crate::error::ArchiveError;
	use crate::limits::MAX_XML_BYTES;
	use crate::limits::MAX_XML_DEPTH;
	use rootcause::Result;
	use tokio_util::sync::CancellationToken;

	const MINIMAL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="ModConfig5.0.xsd">
  <moduleName>Fixture</moduleName>
  <requiredInstallFiles><file source="Data/a.txt" destination="a.txt" /></requiredInstallFiles>
</config>"#;

	#[test]
	fn parses_bounded_fomm_five_document() -> Result<(), ArchiveError> {
		let document = parse(MINIMAL.as_bytes(), &CancellationToken::new())?;
		assert_eq!(document.schema_version, "5.0");
		assert_eq!(document.root.name, "config");
		Ok(())
	}

	#[test]
	fn decodes_predefined_general_references() -> Result<(), ArchiveError> {
		let xml = MINIMAL.replace("Fixture", "Escaped &amp; &lt;");
		let document = parse(xml.as_bytes(), &CancellationToken::new())?;
		assert_eq!(document.root.children[0].text, "Escaped & <");
		Ok(())
	}

	#[test]
	fn decodes_decimal_and_hexadecimal_character_references() -> Result<(), ArchiveError> {
		let xml = MINIMAL.replace("Fixture", "Numeric &#233; &#x3A3;");
		let document = parse(xml.as_bytes(), &CancellationToken::new())?;
		assert_eq!(document.root.children[0].text, "Numeric é Σ");
		Ok(())
	}

	#[test]
	fn rejects_custom_general_references() {
		let xml = MINIMAL.replace("Fixture", "&custom;");
		assert!(parse(xml.as_bytes(), &CancellationToken::new()).is_err());
	}

	#[test]
	fn accepts_bom_aware_utf16() {
		let mut bytes = vec![0xff, 0xfe];
		for word in MINIMAL.replace("UTF-8", "UTF-16").encode_utf16() {
			bytes.extend_from_slice(&word.to_le_bytes());
		}
		assert!(parse(&bytes, &CancellationToken::new()).is_ok());
	}

	#[test]
	fn rejects_dtd_and_processing_instructions() {
		for malicious in [
			MINIMAL.replace(
				"<config",
				"<!DOCTYPE config [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><config",
			),
			MINIMAL.replace("<moduleName>Fixture", "<?target value?><moduleName>Fixture"),
		] {
			assert!(parse(malicious.as_bytes(), &CancellationToken::new()).is_err());
		}
	}

	#[test]
	fn rejects_unknown_semantic_file_operation() {
		let invalid = MINIMAL.replace(
			"<file source=\"Data/a.txt\" destination=\"a.txt\" />",
			"<delete path=\"Data/a.txt\" />",
		);
		assert!(parse(invalid.as_bytes(), &CancellationToken::new()).is_err());
	}

	#[test]
	fn rejects_namespaced_or_unknown_semantics() {
		let cancellation = CancellationToken::new();
		let namespaced = MINIMAL
			.replace("<config xmlns:xsi=", "<x:config xmlns:x=\"urn:unknown\" xmlns:xsi=")
			.replace("</config>", "</x:config>");
		let unknown_attribute = MINIMAL.replace("source=", "unknown=\"value\" source=");
		assert!(parse(namespaced.as_bytes(), &cancellation).is_err());
		assert!(parse(unknown_attribute.as_bytes(), &cancellation).is_err());
	}

	#[test]
	fn rejects_namespace_qualified_semantic_attributes() {
		let cancellation = CancellationToken::new();
		let declared_prefix =
			MINIMAL.replace("<config xmlns:xsi=", r#"<config xmlns:evil="urn:evil" xmlns:xsi="#);
		let hostile_source = declared_prefix.replace("source=", "evil:source=");
		let hostile_operator = declared_prefix.replace(
			"<moduleName>Fixture</moduleName>",
			r#"<moduleName>Fixture</moduleName><moduleDependencies evil:operator="And" />"#,
		);
		let hostile_state = declared_prefix.replace(
			"<moduleName>Fixture</moduleName>",
			concat!(
				"<moduleName>Fixture</moduleName><moduleDependencies>",
				r#"<fileDependency file="a.txt" evil:state="Active" />"#,
				"</moduleDependencies>",
			),
		);
		let undeclared_local_name_masquerade = MINIMAL.replace("source=", "evil:source=");

		for hostile in [
			hostile_source,
			hostile_operator,
			hostile_state,
			undeclared_local_name_masquerade,
		] {
			assert!(parse(hostile.as_bytes(), &cancellation).is_err());
		}
	}

	#[test]
	fn accepts_xsi_schema_location_through_a_namespace_alias() -> Result<(), ArchiveError> {
		let aliased = MINIMAL
			.replace("xmlns:xsi=", "xmlns:schema=")
			.replace("xsi:", "schema:");
		let document = parse(aliased.as_bytes(), &CancellationToken::new())?;
		assert_eq!(document.schema_version, "5.0");
		Ok(())
	}

	#[test]
	fn preserves_exact_flag_text() -> Result<(), ArchiveError> {
		let cancellation = CancellationToken::new();
		let xml = MINIMAL.replace(
			concat!(
				"<requiredInstallFiles>",
				"<file source=\"Data/a.txt\" destination=\"a.txt\" />",
				"</requiredInstallFiles>",
			),
			concat!(
				"<installSteps order=\"Explicit\"><installStep name=\"Step\">",
				"<optionalFileGroups order=\"Explicit\"><group name=\"Group\" type=\"SelectAny\">",
				"<plugins order=\"Explicit\"><plugin name=\"Option\"><conditionFlags>",
				"<flag name=\"exact\">  Value  </flag></conditionFlags>",
				"<typeDescriptor><type name=\"Optional\" /></typeDescriptor>",
				"</plugin></plugins></group></optionalFileGroups></installStep></installSteps>",
			),
		);
		let document = parse(xml.as_bytes(), &cancellation)?;
		let flag = &document.root.children[1].children[0].children[0].children[0].children[0].children[0]
			.children[0]
			.children[0];
		assert_eq!(flag.text, "  Value  ");
		Ok(())
	}

	#[test]
	fn rejects_xml_byte_and_depth_limit_violations() {
		let cancellation = CancellationToken::new();
		let oversized = vec![b' '; MAX_XML_BYTES as usize + 1];
		assert!(parse(&oversized, &cancellation).is_err());

		let mut deep = String::from("<config version=\"5.0\"><moduleName>Name</moduleName>");
		for _ in 0..=MAX_XML_DEPTH {
			deep.push_str("<dependencies>");
		}
		for _ in 0..=MAX_XML_DEPTH {
			deep.push_str("</dependencies>");
		}
		deep.push_str("</config>");
		assert!(parse(deep.as_bytes(), &cancellation).is_err());
	}
}
