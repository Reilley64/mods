use crate::json_output::warning;
use crate::json_values::identity;
use crate::json_values::path;
use application::export::ExportEnvironmentOutput;
use application::export::ExportProvider;
use application::preparation::PluginWarning;
use serde_json::Value;
use serde_json::json;
use std::path::Path;

pub(crate) fn export(output_path: &Path, output: &ExportEnvironmentOutput) -> Value {
	json!({
	    "outcome": if output.published { "published" } else { "preview" },
	    "output": output_path.display().to_string(),
	    "total_bytes": output.total_bytes,
	    "files": output.files.iter().map(|file| json!({
		"path": path(file.path.as_str()), "bytes": file.bytes, "provider": provider(&file.provider),
	    })).collect::<Vec<_>>(),
	})
}

fn provider(value: &ExportProvider) -> Value {
	match value {
		ExportProvider::Data(data_provider) => identity(data_provider),
		ExportProvider::Profile => json!({"kind": "profile"}),
		ExportProvider::GeneratedInvalidation => json!({"kind": "generated_invalidation"}),
	}
}

pub(crate) fn plugin_warnings(values: &[PluginWarning]) -> Vec<Value> {
	values.iter()
		.map(|value| match value {
			PluginWarning::StalePluginEntry { name } => warning(
				"stale_plugin_entry",
				"plugins.txt entry is absent from the analytical Data view",
				json!({"plugin": name}),
			),
			PluginWarning::DuplicatePluginEntry { name } => warning(
				"duplicate_plugin_entry",
				"plugins.txt repeats this entry; the first occurrence counts",
				json!({"plugin": name, "file": "plugins.txt"}),
			),
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::export;
	use super::plugin_warnings;
	use application::ErrorMarker;
	use application::export::ExportEnvironmentOutput;
	use application::export::ExportFile;
	use application::export::ExportProvider;
	use application::preparation::PluginWarning;
	use domain::DataRelativePath;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ProviderIdentity;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;
	use serde_json::json;
	use std::path::Path;

	#[test]
	fn export_lists_files_with_provider_identities_and_plugin_warnings() -> Result<(), ErrorMarker> {
		let file = |path: &str, provider: ExportProvider, bytes: u64| -> Result<ExportFile, ErrorMarker> {
			Ok(ExportFile {
				source_id: 0,
				path: DataRelativePath::new(path.to_owned())
					.context(ErrorMarker::invalid_data_path())?,
				provider,
				bytes,
			})
		};
		let output = ExportEnvironmentOutput {
			files: vec![
				file(
					"Data/mesh.nif",
					ExportProvider::Data(ProviderIdentity::DataMod {
						mod_name: ModName::new("Mesh".to_owned())
							.context(ErrorMarker::invalid_mod_name())?,
						priority: ModPriority::new(7),
					}),
					12,
				)?,
				file(
					"Data/Fallout - Invalidation.bsa",
					ExportProvider::GeneratedInvalidation,
					83,
				)?,
				file("profile/Fallout.ini", ExportProvider::Profile, 5)?,
			],
			total_bytes: 100,
			published: true,
			warnings: Vec::new(),
		};

		let value = export(Path::new("/output"), &output);

		assert_eq!(value["outcome"], "published");
		assert_eq!(value["total_bytes"], 100);
		assert_eq!(
			value["files"][0],
			json!({"path": "Data\\mesh.nif", "bytes": 12, "provider": {"kind": "data_mod", "mod_name": "Mesh", "priority": {"kind": "regular", "priority": 7}}})
		);
		assert_eq!(value["files"][1]["provider"], json!({"kind": "generated_invalidation"}));
		assert_eq!(value["files"][2]["provider"], json!({"kind": "profile"}));
		assert_eq!(
			plugin_warnings(&[PluginWarning::DuplicatePluginEntry { name: "A.esp".into() }])[0]["details"],
			json!({"plugin": "A.esp", "file": "plugins.txt"})
		);
		Ok(())
	}
}
