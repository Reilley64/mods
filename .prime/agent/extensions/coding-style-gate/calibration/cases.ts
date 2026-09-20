export interface CalibrationCase {
	name: string;
	expectedViolation: boolean;
	ruleId: string;
	path?: string;
	referencingFiles?: string[];
	before: string;
	after: string;
}

export const calibrationCases: CalibrationCase[] = [
	{
		name: "reason-comment-bad",
		expectedViolation: true,
		ruleId: "comments-and-documentation-reason-comments",
		before: "fn increment(value: &mut u32) {\n\t*value += 1;\n}\n",
		after: "fn increment(value: &mut u32) {\n\t// Add one to value.\n\t*value += 1;\n}\n",
	},
	{
		name: "reason-comment-good",
		expectedViolation: false,
		ruleId: "comments-and-documentation-reason-comments",
		before: "fn increment(value: &mut u32) {\n\t*value += 1;\n}\n",
		after: "fn increment(value: &mut u32) {\n\t// Steam reports one-based attempts, so preserve the offset in diagnostics.\n\t*value += 1;\n}\n",
	},
	{
		name: "guard-clause-bad",
		expectedViolation: true,
		ruleId: "control-flow-guard-clauses",
		before: "",
		after: "fn publish(ready: bool) -> Result<(), NotReady> {\n\tif ready {\n\t\tdo_publish();\n\t} else {\n\t\treturn Err(NotReady);\n\t}\n\tOk(())\n}\n",
	},
	{
		name: "guard-clause-good",
		expectedViolation: false,
		ruleId: "control-flow-guard-clauses",
		before: "",
		after: "fn publish(ready: bool) -> Result<(), NotReady> {\n\tif !ready {\n\t\treturn Err(NotReady);\n\t}\n\n\tdo_publish();\n\tOk(())\n}\n",
	},
	{
		name: "simple-match-bad",
		expectedViolation: true,
		ruleId: "control-flow-match-only-for-multi-way-logic",
		before: "",
		after: "fn publish(value: Option<Value>) {\n\tmatch value {\n\t\tSome(value) => use_value(value),\n\t\tNone => {},\n\t}\n}\n",
	},
	{
		name: "simple-match-good",
		expectedViolation: false,
		ruleId: "control-flow-match-only-for-multi-way-logic",
		before: "",
		after: "fn publish(value: Option<Value>) {\n\tif let Some(value) = value {\n\t\tuse_value(value);\n\t}\n}\n",
	},
	{
		name: "pointless-helper-bad",
		expectedViolation: true,
		ruleId: "functions-and-tests-helpers-earn-an-interface",
		before: "",
		after: "fn cancellation_requested(cancellation: &CancellationToken) -> bool {\n\tcancellation.is_cancelled()\n}\n\nfn copy(cancellation: &CancellationToken) {\n\tif cancellation_requested(cancellation) {\n\t\treturn;\n\t}\n}\n",
	},
	{
		name: "meaningful-helper-good",
		expectedViolation: false,
		ruleId: "functions-and-tests-helpers-earn-an-interface",
		before: "",
		after: "fn validate_archive_paths(entries: &[ArchiveEntry]) -> Result<ValidatedPaths, InvalidPath> {\n\tlet normalized = normalize_entries(entries)?;\n\treject_traversal(&normalized)?;\n\treject_platform_collisions(&normalized)?;\n\tOk(ValidatedPaths::new(normalized))\n}\n",
	},
	{
		name: "discarded-error-bad",
		expectedViolation: true,
		ruleId: "errors-preserve-causes-at-owned-boundaries",
		before: "",
		after: "async fn read(key: Key) -> Result<Value, ReadSettingError> {\n\tread_value(key).await.map_err(|_| ReadSettingError)\n}\n",
	},
	{
		name: "preserved-error-good",
		expectedViolation: false,
		ruleId: "errors-preserve-causes-at-owned-boundaries",
		before: "",
		after: "async fn read(key: Key) -> rootcause::Result<Value, ReadSettingError> {\n\tread_value(key).await.context(ReadSettingError)\n}\n",
	},
	{
		name: "cancellation-helper-bad",
		expectedViolation: true,
		ruleId: "runtime-primitives-cancellation-propagation-and-checkpoints",
		before: "",
		after: "fn check_cancelled(cancellation: &CancellationToken) -> Result<(), CopyCancelled> {\n\tif cancellation.is_cancelled() {\n\t\treturn Err(CopyCancelled);\n\t}\n\tOk(())\n}\n",
	},
	{
		name: "inline-cancellation-good",
		expectedViolation: false,
		ruleId: "runtime-primitives-cancellation-propagation-and-checkpoints",
		before: "",
		after: "fn copy(cancellation: &CancellationToken) -> rootcause::Result<(), CopyCancelled> {\n\tif cancellation.is_cancelled() {\n\t\treturn Err(CopyCancelled.into_report());\n\t}\n\tcopy_next_file()?;\n\tOk(())\n}\n",
	},
	{
		name: "unsafe-proof-bad",
		expectedViolation: true,
		ruleId: "unsafe-rust-safety-proofs",
		before: "",
		after: "fn open(path: *const u16) -> Handle {\n\t// SAFETY: This call is safe.\n\tunsafe { OpenFile(path) }\n}\n",
	},
	{
		name: "unsafe-proof-good",
		expectedViolation: false,
		ruleId: "unsafe-rust-safety-proofs",
		before: "",
		after: "fn open(path: &WidePath) -> OwnedHandle {\n\t// SAFETY: `path` is NUL-terminated and lives through the call. The result is\n\t// checked for INVALID_HANDLE_VALUE and closed by `OwnedHandle`.\n\tlet handle = unsafe { OpenFile(path.as_ptr()) };\n\tOwnedHandle::try_from(handle)\n}\n",
	},
	{
		name: "parameter-order-bad",
		expectedViolation: true,
		ruleId: "application-use-cases-and-ports-use-case-parameters",
		before: "",
		after: "async fn install(path: PathBuf, cancellation: CancellationToken, dependencies: Dependencies) { }\n",
	},
	{
		name: "parameter-order-good",
		expectedViolation: false,
		ruleId: "application-use-cases-and-ports-use-case-parameters",
		before: "",
		after: "async fn install(dependencies: Dependencies, path: PathBuf, cancellation: CancellationToken) { }\n",
	},
	{
		name: "raw-report-bad",
		expectedViolation: true,
		ruleId: "errors-presentation-error-allowlists",
		before: "",
		after: "fn error_response(report: Report) -> Response {\n\tResponse::error(format!(\"{report:?}\"))\n}\n",
	},
	{
		name: "allowlisted-report-good",
		expectedViolation: false,
		ruleId: "errors-presentation-error-allowlists",
		before: "",
		after: "fn error_response(report: &Report) -> Response {\n\tResponse::error(map_report_to_public_error(report))\n}\n",
	},

	{
		name: "semantic-spacing-bad",
		expectedViolation: true,
		ruleId: "formatting-and-imports-phase-spacing",
		before: "",
		after: "fn install(input: Input) -> Result<Output, Error> {\n\tvalidate(&input)?;\n\tlet staged = stage(input)?;\n\tpublish(&staged)?;\n\tlet output = Output::from(staged);\n\tOk(output)\n}\n",
	},
	{
		name: "semantic-spacing-good",
		expectedViolation: false,
		ruleId: "formatting-and-imports-phase-spacing",
		before: "",
		after: "fn install(input: Input) -> Result<Output, Error> {\n\tvalidate(&input)?;\n\n\tlet staged = stage(input)?;\n\n\tpublish(&staged)?;\n\n\tlet output = Output::from(staged);\n\tOk(output)\n}\n",
	},

	{
		name: "use-case-local-module-bad",
		expectedViolation: true,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/installation/fomod.rs",
		referencingFiles: [
			"src/application/src/installation/install_archive.rs",
			"src/application/src/installation/mod.rs",
		],
		before: "",
		after: "pub(super) fn evaluate(installer: &FomodInstaller) -> Evaluation {\n\tlet ordered_groups = order_groups(&installer.groups);\n\tlet required_options = collect_required_options(&ordered_groups);\n\tEvaluation::new(ordered_groups, required_options)\n}\n",
	},
	{
		name: "use-case-local-module-good",
		expectedViolation: false,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/installation/install_archive/fomod.rs",
		referencingFiles: ["src/application/src/installation/install_archive.rs"],
		before: "",
		after: "pub(super) fn evaluate(installer: &FomodInstaller) -> Evaluation {\n\tlet ordered_groups = order_groups(&installer.groups);\n\tlet required_options = collect_required_options(&ordered_groups);\n\tEvaluation::new(ordered_groups, required_options)\n}\n",
	},
	{
		name: "use-case-local-placeholder-logic-bad",
		expectedViolation: true,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/execution/planning.rs",
		referencingFiles: [
			"src/application/src/execution/execute_program.rs",
			"src/application/src/execution/mod.rs",
		],
		before: "",
		after: "pub(super) fn build_plan(input: &ExecutionInput) -> ExecutionPlan {\n\tExecutionPlan::from(input)\n}\n",
	},
	{
		name: "use-case-local-empty-placeholder-good",
		expectedViolation: false,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/execution/planning.rs",
		referencingFiles: [],
		before: "",
		after: "",
	},
	{
		name: "use-case-entrypoint-helper-bad",
		expectedViolation: true,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/installation/archive_plan.rs",
		referencingFiles: [
			"src/application/src/installation/install_archive.rs",
			"src/application/src/installation/mod.rs",
		],
		before: "pub(super) fn build(entries: &[ArchiveEntry]) -> Plan { Plan::default() }\n",
		after: `pub(super) fn build(entries: &[ArchiveEntry]) -> Plan {
	let selected = entries.iter().filter(|entry| entry.enabled).cloned().collect();
	Plan { selected }
}
`,
	},
	{
		name: "use-case-entrypoint-good",
		expectedViolation: false,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/installation/install_archive.rs",
		referencingFiles: ["src/application/src/installation/mod.rs"],
		before: `pub async fn install_archive(
	dependencies: InstallArchiveDependencies,
	input: InstallArchiveInput,
) -> Result<InstallArchiveOutput, InstallArchiveError> {
	dependencies.archive.install(input.archive).await?;
	Ok(InstallArchiveOutput {})
}
`,
		after: `mod fomod;
mod planning;

pub async fn install_archive(
	dependencies: InstallArchiveDependencies,
	input: InstallArchiveInput,
) -> Result<InstallArchiveOutput, InstallArchiveError> {
	let plan = planning::build(&input.archive)?;
	fomod::install(&dependencies, plan).await?;
	Ok(InstallArchiveOutput {})
}
`,
	},

	{
		name: "capability-interface-helper-bad",
		expectedViolation: true,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/installation/archive_selection.rs",
		referencingFiles: [
			"src/application/src/installation/install_archive.rs",
			"src/application/src/installation/mod.rs",
		],
		before: "",
		after: `pub(super) fn select(entries: Vec<ArchiveEntry>) -> Vec<ArchiveEntry> {
	entries.into_iter().filter(|entry| entry.enabled).collect()
}
`,
	},
	{
		name: "capability-interface-types-good",
		expectedViolation: false,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/installation/types.rs",
		referencingFiles: [
			"src/application/src/installation/install_archive.rs",
			"src/application/src/installation/mod.rs",
		],
		before: "pub struct InstallArchiveInput;\n",
		after: `pub struct InstallArchiveInput {
	pub archive: Archive,
}

pub struct InstallArchiveOutput {
	pub installed_files: Vec<PathBuf>,
}
`,
	},

	{
		name: "rustdoc-format-bad",
		expectedViolation: true,
		ruleId: "comments-and-documentation-rustdoc-format",
		before: "",
		after: `// Installs an archive and returns an error when extraction fails.
pub async fn install_archive(input: InstallArchiveInput) -> Result<InstallArchiveOutput, InstallArchiveError> {
	normalize_and_install(input).await
}
`,
	},
	{
		name: "rustdoc-format-good",
		expectedViolation: false,
		ruleId: "comments-and-documentation-rustdoc-format",
		before: "",
		after: `/// Installs an archive.
///
/// # Errors
///
/// Returns [\`InstallArchiveError\`] when extraction fails.
pub async fn install_archive(input: InstallArchiveInput) -> Result<InstallArchiveOutput, InstallArchiveError> {
	// The provider requires paths to be normalized before extraction.
	normalize_and_install(input).await
}
`,
	},

	{
		name: "use-case-local-application-scope-bad",
		expectedViolation: true,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/application/src/game/file_version.rs",
		referencingFiles: ["src/application/src/game/mod.rs", "src/application/src/game/synchronize.rs"],
		before: "",
		after: `pub(super) fn read_version(path: &Path) -> Result<Version, ReadVersionError> {
	let bytes = fs::read(path)?;
	Version::parse(&bytes)
}
`,
	},
	{
		name: "use-case-local-non-application-scope-good",
		expectedViolation: false,
		ruleId: "application-use-cases-and-ports-use-case-local-implementation-modules",
		path: "src/infrastructure/game_platform/src/file_version.rs",
		referencingFiles: [
			"src/infrastructure/game_platform/src/adapter.rs",
			"src/infrastructure/game_platform/src/lib.rs",
		],
		before: "",
		after: `pub(crate) fn read_version(path: &Path) -> Result<Version, ReadVersionError> {
	let bytes = fs::read(path)?;
	Version::parse(&bytes)
}
`,
	},

];
