mod inventory;
use super::ExportFile;
use super::PrepareExport;
use inventory::plan_inventory;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::fmt;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub struct ExportEnvironmentDependencies {
	pub prepare_export: PrepareExport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEnvironmentOutput {
	pub files: Vec<ExportFile>,
	pub total_bytes: u64,
	pub published: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ExportEnvironmentError;
impl fmt::Display for ExportEnvironmentError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to export environment")
	}
}

#[tracing::instrument(skip_all)]
pub async fn export_environment(
	dependencies: ExportEnvironmentDependencies,
	output: PathBuf,
	include_saves: bool,
	dry_run: bool,
	cancellation: CancellationToken,
) -> Result<ExportEnvironmentOutput, ExportEnvironmentError> {
	let prepared = dependencies
		.prepare_export
		.call((output, include_saves, cancellation.clone()))
		.await
		.context(ExportEnvironmentError)?;

	let (files, total_bytes) = plan_inventory(prepared.files)?;

	if !dry_run {
		prepared.publish
			.call((files.clone(), cancellation))
			.await
			.context(ExportEnvironmentError)?;
	}

	Ok(ExportEnvironmentOutput {
		files,
		total_bytes,
		published: !dry_run,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ErrorMarker;
	use crate::export::ExportProvider;
	use crate::export::PreparedExport;
	use crate::ports::PortFuture;
	use domain::DataRelativePath;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ProviderIdentity;
	use rootcause::report;
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;
	use std::sync::atomic::Ordering;

	fn entry(id: usize, path: &str, provider: ExportProvider) -> Result<ExportFile, ErrorMarker> {
		Ok(ExportFile {
			source_id: id,
			path: DataRelativePath::new(path.to_owned()).context(ErrorMarker::invalid_data_path())?,
			provider,
			bytes: 4,
		})
	}

	#[tokio::test]
	async fn dry_run_excludes_base_and_uses_highest_directory_spelling_without_publication()
	-> Result<(), ErrorMarker> {
		let published = Arc::new(AtomicBool::new(false));
		let observed = published.clone();
		let files = vec![
			entry(0, "Data/Base.esm", ExportProvider::Data(ProviderIdentity::SteamData))?,
			entry(
				1,
				"Data/Textures/Low.dds",
				ExportProvider::Data(ProviderIdentity::DataMod {
					mod_name: ModName::new("Mod".to_owned())
						.context(ErrorMarker::invalid_mod_name())?,
					priority: ModPriority::new(0),
				}),
			)?,
			entry(
				2,
				"Data/TEXTURES/High.dds",
				ExportProvider::Data(ProviderIdentity::Overwrite),
			)?,
			entry(3, "profile/Fallout.ini", ExportProvider::Profile)?,
		];
		let dependencies = ExportEnvironmentDependencies {
			prepare_export: Arc::new(move |_, saves, _| {
				assert!(!saves);
				let files = files.clone();
				let observed = observed.clone();
				Box::pin(async move {
					Ok(PreparedExport {
						files,
						publish: Arc::new(move |_, _| {
							observed.store(true, Ordering::SeqCst);
							Box::pin(async { Ok(()) }) as PortFuture<_>
						}),
					})
				}) as PortFuture<_>
			}),
		};
		let output = export_environment(
			dependencies,
			PathBuf::from("/output"),
			false,
			true,
			CancellationToken::new(),
		)
		.await
		.context(ErrorMarker::io_failure())?;
		assert_eq!(output.total_bytes, 12);
		assert!(!output.published);
		assert!(!published.load(Ordering::SeqCst));
		assert!(output
			.files
			.iter()
			.any(|file| file.path.as_str() == "Data/TEXTURES/Low.dds"));
		assert!(!output.files.iter().any(|file| file.path.as_str().contains("Base")));
		Ok(())
	}

	#[tokio::test]
	async fn structural_conflicts_fail_before_publication() -> Result<(), ErrorMarker> {
		let files = vec![
			entry(0, "Data/Meshes", ExportProvider::Data(ProviderIdentity::Overwrite))?,
			entry(
				1,
				"Data/meshes/file.nif",
				ExportProvider::Data(ProviderIdentity::Overwrite),
			)?,
		];
		let dependencies = ExportEnvironmentDependencies {
			prepare_export: Arc::new(move |_, _, _| {
				let files = files.clone();
				Box::pin(async move {
					Ok(PreparedExport {
						files,
						publish: Arc::new(|_, _| {
							Box::pin(async { Err(report!(ErrorMarker::io_failure())) })
								as PortFuture<_>
						}),
					})
				}) as PortFuture<_>
			}),
		};
		assert!(export_environment(
			dependencies,
			PathBuf::from("/output"),
			false,
			false,
			CancellationToken::new()
		)
		.await
		.is_err());
		Ok(())
	}
}
