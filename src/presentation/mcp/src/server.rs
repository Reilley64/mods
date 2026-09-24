use crate::commands::LogLevel;
use crate::conflict_output::explanation;
use crate::conflict_output::inspection;
use crate::conflict_output::list;
use crate::contract::Contracts;
use crate::diagnostics::DiagnosticSession;
use crate::diagnostics::SINK_WARNING;
use crate::diagnostics::SessionStart;
use crate::error_output::application_error;
use crate::execution_output::executed;
use crate::inputs::Exec;
use crate::inputs::Install;
use crate::install_output::additional_selections;
use crate::install_output::installed;
use crate::install_output::preview;
use crate::install_output::warning as install_warning;
use crate::lifecycle::Lifecycle;
use crate::path_resolution::resolve_path;
use crate::settings_output::mutation;
use crate::settings_output::setting;
use application::ErrorMarker;
use application::conflicts::explain_path;
use application::conflicts::inspect_mod_conflicts;
use application::conflicts::list_effective_conflicts;
use application::execution::execute_program;
use application::installation::InstallArchiveOutput;
use application::installation::install_archive;
use application::ports::ProgressEvent;
use application::ports::ReportProgress;
use application::settings::SettingKey;
use application::settings::get_setting;
use application::settings::list_settings;
use application::settings::set_game_directory;
use domain::ArchivePath;
use domain::DataRelativePath;
use domain::EnvironmentRoot;
use domain::FomodChoice;
use domain::GameInstallationPath;
use domain::ModName;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use infrastructure::Resources;
use rmcp::ErrorData;
use rmcp::RoleServer;
use rmcp::ServerHandler;
use rmcp::model::CacheScope;
use rmcp::model::CallToolRequestMethod;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResponse;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::ListToolsResult;
use rmcp::model::MetaObject;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ProgressNotificationParam;
use rmcp::model::ProtocolVersion;
use rmcp::model::RequestId;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerInfo;
use rmcp::service::RequestContext;
use rootcause::prelude::ResultExt;
use serde_json::Value;
use serde_json::from_value;
use serde_json::json;
use std::borrow::Cow;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct Server {
	resources: Resources,
	contracts: Arc<Contracts>,
	lifecycle: Arc<Lifecycle>,
	admission: Arc<Semaphore>,
	root: EnvironmentRoot,
	startup: PathBuf,
	log_level: LogLevel,
}

impl Server {
	pub(crate) fn new(
		resources: Resources,
		root: EnvironmentRoot,
		startup: PathBuf,
		log_level: LogLevel,
	) -> rootcause::Result<Self> {
		Ok(Self {
			resources,
			root,
			startup,
			log_level,
			contracts: Arc::new(Contracts::new()?),
			lifecycle: Arc::new(Lifecycle::default()),
			admission: Arc::new(Semaphore::new(1)),
		})
	}

	pub(crate) fn lifecycle(&self) -> Arc<Lifecycle> {
		self.lifecycle.clone()
	}

	pub(crate) async fn run_tool(
		&self,
		name: &str,
		arguments: Value,
		progress: Option<ReportProgress>,
		request_id: Option<RequestId>,
		cancellation: CancellationToken,
	) -> Result<CallToolResult, ErrorData> {
		if self.lifecycle.shutdown.is_cancelled() {
			return Err(ErrorData::internal_error("server is shutting down", None));
		}

		let admission_permit = match self.admission.clone().try_acquire_owned() {
			Ok(permit) => permit,
			Err(_) => {
				return Ok(CallToolResult::structured_error(json!({"outcome": "error", "error": {
					"code": "environment_busy", "message": "environment is busy", "retryable": true
				}})));
			}
		};

		let problems = self
			.contracts
			.validate(name, &arguments)
			.map_err(|_| ErrorData::invalid_params("unknown tool", None))?;
		if problems.as_array().is_some_and(|problems| !problems.is_empty()) {
			return Ok(invalid_arguments(problems));
		}

		if cancellation.is_cancelled() {
			return Err(ErrorData::internal_error("request cancelled", None));
		}

		let operation_name = match name {
			"mods_config_list" => "config.list",
			"mods_config_get" => "config.get",
			"mods_config_set_game_dir" => "config.set.game_dir",
			"mods_install" => "install",
			"mods_conflicts_list" => "conflicts.list",
			"mods_conflicts_inspect" => "conflicts.inspect",
			"mods_conflicts_explain" => "conflicts.explain",
			"mods_exec" => "exec",
			_ => return Err(ErrorData::invalid_params("unknown tool", None)),
		};
		let session = DiagnosticSession::start(self.root.as_path(), self.log_level, operation_name);
		let operation = async {
			match name {
				"mods_exec" => {
					let input: Exec = from_value(arguments.clone()).map_err(|_| {
						ErrorData::internal_error("invalid validated execution", None)
					})?;
					let target = match input
						.output_target
						.map(ModName::new)
						.transpose()
						.context(ErrorMarker::invalid_output_target())
					{
						Ok(Some(name)) => OutputTarget::DataMod(name),
						Ok(None) => OutputTarget::Overwrite,
						Err(report) => return application_error(name, &report),
					};
					let program = match Program::new(input.program.into())
						.context(ErrorMarker::program_unsupported())
					{
						Ok(program) => program,
						Err(report) => return application_error(name, &report),
					};
					let args = match input
						.args
						.into_iter()
						.map(|value| ProgramArgument::new(value.into()))
						.collect::<rootcause::Result<Vec<_>, _>>()
						.context(ErrorMarker::program_unsupported())
					{
						Ok(args) => args,
						Err(report) => return application_error(name, &report),
					};
					let directory = match input
						.cwd
						.map(|value| {
							let value = WorkingDirectory::new(value.into())?;
							WorkingDirectory::new(resolve_path(
								value.as_path(),
								&self.startup,
							))
						})
						.transpose()
						.context(ErrorMarker::invalid_working_directory())
					{
						Ok(directory) => directory,
						Err(report) => return application_error(name, &report),
					};

					let (mut dependencies, capture) =
						self.resources.captured_execution_dependencies(
							self.startup.clone(),
							self.lifecycle.shutdown.child_token(),
						);
					dependencies.report_progress = progress.clone();
					if let Some(id) = &request_id {
						self.lifecycle
							.retain_capture(id.clone(), capture.clone(), admission_permit)
							.await;
					}

					let output = execute_program(
						dependencies,
						target,
						directory,
						program,
						args,
						cancellation.clone(),
					)
					.await;
					if let Some(id) = &request_id {
						self.lifecycle
							.execution_finished(id.clone(), cancellation.clone())
							.await;
					}

					let output = match output {
						Ok(output) => output,
						Err(report) => return application_error(name, &report),
					};

					match capture.read() {
						Ok(captured) => Ok(executed(output, captured)),
						Err(report) => application_error(name, &report),
					}
				}

				"mods_config_list" => {
					let mut dependencies = self.resources.list_settings_dependencies();
					dependencies.report_progress = progress.clone();

					match list_settings(dependencies).await {
						Ok(output) => Ok(CallToolResult::structured(
							json!({"outcome": "complete", "settings": output.settings.iter().map(setting).collect::<Vec<_>>()}),
						)),
						Err(report) => application_error(name, &report),
					}
				}
				"mods_config_get" => {
					let key = SettingKey::ALL
						.into_iter()
						.find(|key| arguments["key"].as_str() == Some(key.manifest_path()))
						.ok_or_else(|| {
							ErrorData::internal_error("invalid validated setting key", None)
						})?;

					let mut dependencies = self.resources.get_setting_dependencies();
					dependencies.report_progress = progress.clone();

					match get_setting(dependencies, key).await {
						Ok(output) => Ok(CallToolResult::structured(
							json!({"outcome": "complete", "setting": setting(&output.setting)}),
						)),
						Err(report) => application_error(name, &report),
					}
				}
				"mods_config_set_game_dir" => {
					let supplied = arguments["game_dir"].as_str().ok_or_else(|| {
						ErrorData::internal_error("invalid validated game directory", None)
					})?;
					let path = match GameInstallationPath::new(resolve_path(
						Path::new(supplied),
						&self.startup,
					))
					.context(ErrorMarker::setting_value_invalid())
					{
						Ok(path) => path,
						Err(report) => return application_error(name, &report),
					};

					let mut dependencies = self.resources.set_game_directory_dependencies();
					dependencies.report_progress = progress.clone();

					match set_game_directory(dependencies, path, cancellation.clone()).await {
						Ok(output) => Ok(mutation(&output)),
						Err(report) => application_error(name, &report),
					}
				}
				"mods_install" => {
					let input: Install = from_value(arguments.clone()).map_err(|_| {
						ErrorData::internal_error("invalid validated installation", None)
					})?;
					let archive = match ArchivePath::new(resolve_path(
						Path::new(&input.archive),
						&self.startup,
					))
					.context(ErrorMarker::unsafe_archive().with_phase("archive_validation"))
					{
						Ok(value) => value,
						Err(report) => return application_error(name, &report),
					};
					let mod_name = match input
						.mod_name
						.map(ModName::new)
						.transpose()
						.context(ErrorMarker::invalid_mod_name())
					{
						Ok(value) => value,
						Err(report) => return application_error(name, &report),
					};
					let choices = input
						.choices
						.into_iter()
						.map(|choice| FomodChoice {
							group_id: choice.group_id,
							option_id: choice.option_id,
						})
						.collect();

					let mut dependencies = self.resources.install_archive_dependencies();
					dependencies.report_progress = progress.clone();

					match install_archive(
						dependencies,
						archive,
						mod_name,
						input.replace,
						choices,
						input.dry_run,
						cancellation.clone(),
					)
					.await
					{
						Ok(InstallArchiveOutput::AdditionalSelectionsRequired(output)) => {
							Ok(CallToolResult::structured(additional_selections(&output)?))
						}
						Ok(InstallArchiveOutput::Preview(output)) => {
							Ok(CallToolResult::structured(preview(&output)?))
						}
						Ok(InstallArchiveOutput::Installed(output)) => {
							let mut result =
								CallToolResult::structured(installed(&output)?);
							result.content.extend(output.warnings.iter().map(|warning| {
								ContentBlock::text(install_warning(warning).to_string())
							}));

							Ok(result)
						}
						Err(report) => application_error(name, &report),
					}
				}
				"mods_conflicts_list" => {
					let compare_content = arguments["compare_content"].as_bool().unwrap_or(false);

					let mut dependencies = self.resources.list_effective_conflicts_dependencies();
					dependencies.report_progress = progress.clone();

					match list_effective_conflicts(
						dependencies,
						compare_content,
						cancellation.clone(),
					)
					.await
					{
						Ok(output) => Ok(CallToolResult::structured(list(&output))),
						Err(report) => application_error(name, &report),
					}
				}
				"mods_conflicts_inspect" => {
					let supplied = arguments["mod_name"].as_str().ok_or_else(|| {
						ErrorData::internal_error("invalid validated mod name", None)
					})?;
					let mod_name = match ModName::new(supplied.to_owned())
						.context(ErrorMarker::invalid_mod_name())
					{
						Ok(name) => name,
						Err(report) => return application_error(name, &report),
					};
					let compare_content = arguments["compare_content"].as_bool().unwrap_or(false);

					let mut dependencies = self.resources.inspect_mod_conflicts_dependencies();
					dependencies.report_progress = progress.clone();

					match inspect_mod_conflicts(
						dependencies,
						mod_name,
						compare_content,
						cancellation.clone(),
					)
					.await
					{
						Ok(output) => Ok(CallToolResult::structured(inspection(&output))),
						Err(report) => {
							let mut result = application_error(name, &report)?;
							if let Some(value) = &mut result.structured_content
								&& value["error"]["code"] == "mod_not_found"
							{
								value["error"]["details"] =
									json!({"mod_name": supplied});
								result =
									CallToolResult::structured_error(value.clone());
							}

							Ok(result)
						}
					}
				}
				"mods_conflicts_explain" => {
					let supplied = arguments["path"].as_str().ok_or_else(|| {
						ErrorData::internal_error("invalid validated data path", None)
					})?;
					let path = match DataRelativePath::new(supplied.to_owned())
						.context(ErrorMarker::invalid_data_path())
					{
						Ok(path) => path,
						Err(report) => return application_error(name, &report),
					};
					let compare_content = arguments["compare_content"].as_bool().unwrap_or(false);

					let mut dependencies = self.resources.explain_path_dependencies();
					dependencies.report_progress = progress.clone();

					match explain_path(dependencies, path, compare_content, cancellation.clone())
						.await
					{
						Ok(output) => Ok(CallToolResult::structured(explanation(&output))),
						Err(report) => application_error(name, &report),
					}
				}
				_ => Err(ErrorData::method_not_found::<CallToolRequestMethod>()),
			}
		};

		let result = match session {
			SessionStart::Disabled => operation.await,
			SessionStart::SetupFailed => {
				eprintln!("{SINK_WARNING}");
				operation.await
			}
			SessionStart::FileBacked(session) => {
				let id = session.id();
				let mut result = session.capture(operation).await;

				if let Ok(result) = &mut result {
					result.meta = Some(MetaObject(
						[("diagnostic_session".to_owned(), json!(id.to_string()))]
							.into_iter()
							.collect(),
					));
				}

				let outcome = if cancellation.is_cancelled() {
					"cancelled"
				} else if result.as_ref().is_ok_and(|result| result.is_error != Some(true)) {
					"success"
				} else {
					"error"
				};

				session.finish(outcome);

				result
			}
		};

		let result = result?;
		if let Some(content) = &result.structured_content
			&& !self.contracts.output_matches(name, content)
		{
			return Err(ErrorData::internal_error("tool output violated its contract", None));
		}

		Ok(result)
	}
}

impl ServerHandler for Server {
	fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
		Cow::Owned(vec![ProtocolVersion::V_2026_07_28])
	}

	fn get_info(&self) -> ServerInfo {
		ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
	}

	async fn list_tools(
		&self,
		_: Option<PaginatedRequestParams>,
		_: RequestContext<RoleServer>,
	) -> Result<ListToolsResult, ErrorData> {
		Ok(ListToolsResult::with_all_items(self.contracts.tools())
			.with_ttl_ms(0)
			.with_cache_scope(CacheScope::Private))
	}

	async fn call_tool(
		&self,
		request: CallToolRequestParams,
		context: RequestContext<RoleServer>,
	) -> Result<CallToolResponse, ErrorData> {
		let progress = context.meta.get_progress_token().map(|token| {
			let peer = context.peer.clone();
			let ordinal = Arc::new(Mutex::new(0u64));
			Arc::new(move |event| -> Pin<Box<dyn Future<Output = ()> + Send>> {
				let peer = peer.clone();
				let token = token.clone();
				let ordinal = ordinal.clone();
				Box::pin(async move {
					let mut ordinal = ordinal.lock().await;
					*ordinal += 1;
					let _ = peer
						.notify_progress(
							ProgressNotificationParam::new(token, *ordinal as f64)
								.with_message(match event {
									ProgressEvent::SettingsLoaded => {
										"Settings loaded"
									}
									ProgressEvent::ValidatingGameBinding => {
										"Validating game binding"
									}
									ProgressEvent::GameBindingStored => {
										"Game binding stored"
									}
									ProgressEvent::ScanningArchive => {
										"Scanning archive"
									}
									ProgressEvent::ArchiveIndexed => {
										"Archive indexed"
									}
									ProgressEvent::ArchiveScanCheckpoint => {
										"Archive scan checkpoint"
									}
									ProgressEvent::EvaluatingInstaller => {
										"Evaluating installer"
									}
									ProgressEvent::InstallationPlanned => {
										"Installation planned"
									}
									ProgressEvent::ScanningConflicts => {
										"Scanning conflicts"
									}
									ProgressEvent::ConflictsScanned => {
										"Conflicts scanned"
									}
									ProgressEvent::ExtractingFiles => {
										"Extracting files"
									}
									ProgressEvent::FilesExtracted => {
										"Files extracted"
									}
									ProgressEvent::ExtractionCheckpoint => {
										"Extraction checkpoint"
									}
									ProgressEvent::InstallationPublished => {
										"Installation published"
									}
									ProgressEvent::PreparingExecution => {
										"Preparing execution"
									}
									ProgressEvent::ExecutionPrepared => {
										"Execution prepared"
									}
									ProgressEvent::ExecutionFinished => {
										"Execution finished"
									}
								}),
						)
						.await;
				})
			}) as ReportProgress
		});
		let arguments = Value::Object(request.arguments.unwrap_or_default());
		let server = self.clone();
		let shutdown = self.lifecycle.shutdown.clone();

		self.lifecycle
			.tasks
			.spawn(async move {
				let cancellation = context.ct;
				let operation = server.run_tool(
					&request.name,
					arguments,
					progress,
					Some(context.id),
					cancellation.clone(),
				);
				tokio::pin!(operation);
				tokio::select! {
				    result = &mut operation => result,
				    () = shutdown.cancelled() => { cancellation.cancel(); operation.await }
				}
			})
			.await
			.map_err(|_| ErrorData::internal_error("operation supervision failed", None))?
			.map(Into::into)
	}
}

fn invalid_arguments(problems: Value) -> CallToolResult {
	CallToolResult::structured_error(json!({"outcome": "error", "error": {
		"code": "invalid_arguments", "message": "tool arguments are invalid", "retryable": false,
		"details": {"problems": problems}
	}}))
}

#[cfg(test)]
mod tests {
	use super::Server;
	use super::invalid_arguments;
	use crate::commands::LogLevel;
	use domain::EnvironmentRoot;
	use infrastructure::ExecutionCapture;
	use infrastructure::Resources;
	use rmcp::model::RequestId;
	use rootcause::Result;
	use serde_json::json;
	use serde_json::to_string;
	use std::sync::Arc;
	use std::time::Duration;
	use tempfile::TempDir;
	use tokio::time::timeout;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn invalid_input_is_a_tool_error_without_rejected_values() {
		let result = invalid_arguments(json!([{"field": "archive", "kind": "missing"}]));

		assert_eq!(result.is_error, Some(true));
		assert_eq!(
			result.structured_content,
			Some(json!({"outcome": "error", "error": {
				"code": "invalid_arguments", "message": "tool arguments are invalid", "retryable": false,
				"details": {"problems": [{"field": "archive", "kind": "missing"}]}
			}}))
		);
	}
	#[tokio::test]
	async fn busy_admission_precedes_validation_and_releases_without_queueing() -> Result<()> {
		let root = TempDir::new()?;
		let absent = root.path().join("not-created");
		let server = Server::new(
			Resources::system(EnvironmentRoot::new(absent.clone())?),
			EnvironmentRoot::new(absent.clone())?,
			root.path().to_owned(),
			LogLevel::Off,
		)?;
		let permit = server.admission.clone().try_acquire_owned()?;

		let busy = server
			.run_tool(
				"mods_config_list",
				json!({"unknown": true}),
				None,
				None,
				CancellationToken::new(),
			)
			.await?;
		assert_eq!(
			busy.structured_content,
			Some(json!({"outcome": "error", "error": {
				"code": "environment_busy", "message": "environment is busy", "retryable": true
			}}))
		);
		assert!(!absent.exists());

		drop(permit);
		let admitted = server
			.run_tool(
				"mods_config_list",
				json!({"unknown": true}),
				None,
				None,
				CancellationToken::new(),
			)
			.await?;
		assert_eq!(
			admitted.structured_content
				.as_ref()
				.map(|value| &value["error"]["code"]),
			Some(&json!("invalid_arguments"))
		);
		assert!(!absent.exists());

		Ok(())
	}

	#[tokio::test]
	async fn invalid_arguments_do_not_touch_the_environment() -> Result<()> {
		let root = TempDir::new()?;
		let absent = root.path().join("not-created");
		let server = Server::new(
			Resources::system(EnvironmentRoot::new(absent.clone())?),
			EnvironmentRoot::new(absent.clone())?,
			root.path().to_owned(),
			LogLevel::Off,
		)?;

		let result = server
			.run_tool(
				"mods_config_list",
				json!({"secret": "not echoed"}),
				None,
				None,
				CancellationToken::new(),
			)
			.await?;

		assert_eq!(result.is_error, Some(true));
		assert!(!absent.exists());
		assert!(!to_string(&result)?.contains("not echoed"));

		Ok(())
	}
	#[tokio::test]
	async fn cancelled_completed_execution_admits_next_operation_before_eof() -> Result<()> {
		let root = TempDir::new()?;
		let environment = EnvironmentRoot::new(root.path().join("unused"))?;
		let server = Server::new(
			Resources::system(environment.clone()),
			environment,
			root.path().to_owned(),
			LogLevel::Off,
		)?;
		let cancellation = CancellationToken::new();
		let id = RequestId::Number(42);
		let capture = Arc::new(ExecutionCapture::new(root.path()));
		let weak = Arc::downgrade(&capture);

		server.lifecycle
			.retain_capture(id.clone(), capture, server.admission.clone().try_acquire_owned()?)
			.await;
		server.lifecycle.execution_finished(id, cancellation.clone()).await;

		cancellation.cancel();
		let permit = timeout(Duration::from_secs(1), server.admission.clone().acquire_owned()).await??;
		drop(permit);
		let result = server
			.run_tool(
				"mods_config_list",
				json!({"unknown": true}),
				None,
				None,
				CancellationToken::new(),
			)
			.await?;

		assert_eq!(
			result.structured_content.as_ref().map(|value| &value["error"]["code"]),
			Some(&json!("invalid_arguments"))
		);
		assert!(weak.upgrade().is_none());
		assert!(!server.lifecycle.shutdown.is_cancelled());

		Ok(())
	}
}
