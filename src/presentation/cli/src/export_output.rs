use crate::output::quote;
use application::export::ExportEnvironmentOutput;
use application::export::ExportProvider;
use domain::ProviderIdentity;
use std::fmt::Write;
use std::path::Path;

pub(crate) fn preview(path: &Path, output: &ExportEnvironmentOutput) -> String {
	let mut text = String::from("outcome = \"preview\"\n");
	let _ = writeln!(text, "output = {}", quote(&path.display().to_string()));
	let _ = writeln!(text, "files.count = {}", output.files.len());
	let _ = writeln!(text, "total_bytes = {}", output.total_bytes);

	for (index, file) in output.files.iter().enumerate() {
		let prefix = format!("files[{index}]");
		let _ = writeln!(
			text,
			"{prefix}.path = {}",
			quote(&file.path.as_str().replace('/', "\\"))
		);
		let _ = writeln!(text, "{prefix}.bytes = {}", file.bytes);
		match &file.provider {
			ExportProvider::Data(ProviderIdentity::SteamData) => {
				let _ = writeln!(text, "{prefix}.provider.kind = \"steam_data\"");
			}
			ExportProvider::Data(ProviderIdentity::DataMod { mod_name, priority }) => {
				let _ = writeln!(text, "{prefix}.provider.kind = \"data_mod\"");
				let _ = writeln!(text, "{prefix}.provider.mod_name = {}", quote(mod_name.as_str()));
				let _ = writeln!(text, "{prefix}.provider.priority = {}", priority.get());
			}
			ExportProvider::Data(ProviderIdentity::Overwrite) => {
				let _ = writeln!(text, "{prefix}.provider.kind = \"overwrite\"");
			}
			ExportProvider::Profile => {
				let _ = writeln!(text, "{prefix}.provider.kind = \"profile\"");
			}
			ExportProvider::GeneratedInvalidation => {
				let _ = writeln!(text, "{prefix}.provider.kind = \"generated_invalidation\"");
			}
		}
	}

	text
}

#[cfg(test)]
mod tests {
	use super::preview;
	use application::ErrorMarker;
	use application::export::ExportEnvironmentOutput;
	use application::export::ExportFile;
	use application::export::ExportProvider;
	use domain::DataRelativePath;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ProviderIdentity;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;
	use std::path::Path;

	#[test]
	fn preview_lists_payload_paths_providers_counts_and_bytes() -> Result<(), ErrorMarker> {
		let output = ExportEnvironmentOutput {
			files: vec![
				ExportFile {
					source_id: 0,
					path: DataRelativePath::new("Data/mesh.nif".to_owned())
						.context(ErrorMarker::invalid_data_path())?,
					provider: ExportProvider::Data(ProviderIdentity::DataMod {
						mod_name: ModName::new("Mesh".to_owned())
							.context(ErrorMarker::invalid_mod_name())?,
						priority: ModPriority::new(7),
					}),
					bytes: 12,
				},
				ExportFile {
					source_id: 1,
					path: DataRelativePath::new("profile/saves/save.fos".to_owned())
						.context(ErrorMarker::invalid_data_path())?,
					provider: ExportProvider::Profile,
					bytes: 8,
				},
				ExportFile {
					source_id: 2,
					path: DataRelativePath::new("Data/Fallout - Invalidation.bsa".to_owned())
						.context(ErrorMarker::invalid_data_path())?,
					provider: ExportProvider::GeneratedInvalidation,
					bytes: 3,
				},
			],
			total_bytes: 23,
			published: false,
			warnings: Vec::new(),
		};

		let text = preview(Path::new("/output"), &output);
		for value in [
			"output = \"/output\"",
			"files.count = 3",
			"total_bytes = 23",
			"files[0].path = \"Data\\\\mesh.nif\"",
			"files[0].provider.mod_name = \"Mesh\"",
			"files[0].provider.priority = 7",
			"files[1].provider.kind = \"profile\"",
			"files[2].provider.kind = \"generated_invalidation\"",
		] {
			assert!(text.contains(value), "missing {value}");
		}
		Ok(())
	}
}
