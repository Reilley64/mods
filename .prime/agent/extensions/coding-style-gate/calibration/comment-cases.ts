import type { CalibrationCase } from "./cases";

export const commentCalibrationCases: CalibrationCase[] = [
	{
		"name": "comment-second-interrupt-good",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "pub fn with_force_cancellation(mut self, cancellation: CancellationToken) -> Self {\n    self.force_cancellation = cancellation;\n    self\n}\n",
		"after": "/// Supplies the second console interrupt separately from cooperative cancellation.\npub fn with_force_cancellation(mut self, cancellation: CancellationToken) -> Self {\n    self.force_cancellation = cancellation;\n    self\n}\n",
		"source": {
			"commit": "7d7d939",
			"path": "src/infrastructure/dependencies/src/execution_adapter.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-second-interrupt-bad",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": true,
		"before": "pub fn with_force_cancellation(mut self, cancellation: CancellationToken) -> Self {\n    self.force_cancellation = cancellation;\n    self\n}\n",
		"after": "/// Sets force_cancellation to cancellation and returns self.\npub fn with_force_cancellation(mut self, cancellation: CancellationToken) -> Self {\n    self.force_cancellation = cancellation;\n    self\n}\n",
		"source": {
			"commit": "7d7d939",
			"path": "src/infrastructure/dependencies/src/execution_adapter.rs",
			"kind": "controlled-mutation"
		}
	},
	{
		"name": "comment-child-status-good",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct ProcessStatus {\n\tvalue: u32,\n\torigin: ProcessStatusOrigin,\n}\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum ProcessStatusOrigin {\n\tChild,\n}\nimpl ProcessStatus {\n\tpub const fn new(value: u32) -> Self {\n\t\tSelf {\n\t\t\tvalue,\n\t\t\torigin: ProcessStatusOrigin::Child,\n\t\t}\n\t}\n\tpub const fn value(self) -> u32 {\n\t\tself.value\n\t}\n\tpub const fn origin(self) -> ProcessStatusOrigin {\n\t\tself.origin\n\t}\n}\n",
		"after": "/// The complete Windows root-process status; a nonzero value is still a child result.\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct ProcessStatus {\n\tvalue: u32,\n\torigin: ProcessStatusOrigin,\n}\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum ProcessStatusOrigin {\n\tChild,\n}\nimpl ProcessStatus {\n\tpub const fn new(value: u32) -> Self {\n\t\tSelf {\n\t\t\tvalue,\n\t\t\torigin: ProcessStatusOrigin::Child,\n\t\t}\n\t}\n\tpub const fn value(self) -> u32 {\n\t\tself.value\n\t}\n\tpub const fn origin(self) -> ProcessStatusOrigin {\n\t\tself.origin\n\t}\n}\n",
		"source": {
			"commit": "7d7d939",
			"path": "src/domain/src/execution.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-child-status-bad",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": true,
		"before": "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct ProcessStatus {\n\tvalue: u32,\n\torigin: ProcessStatusOrigin,\n}\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum ProcessStatusOrigin {\n\tChild,\n}\nimpl ProcessStatus {\n\tpub const fn new(value: u32) -> Self {\n\t\tSelf {\n\t\t\tvalue,\n\t\t\torigin: ProcessStatusOrigin::Child,\n\t\t}\n\t}\n\tpub const fn value(self) -> u32 {\n\t\tself.value\n\t}\n\tpub const fn origin(self) -> ProcessStatusOrigin {\n\t\tself.origin\n\t}\n}\n",
		"after": "/// A process status containing a value and an origin.\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct ProcessStatus {\n\tvalue: u32,\n\torigin: ProcessStatusOrigin,\n}\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum ProcessStatusOrigin {\n\tChild,\n}\nimpl ProcessStatus {\n\tpub const fn new(value: u32) -> Self {\n\t\tSelf {\n\t\t\tvalue,\n\t\t\torigin: ProcessStatusOrigin::Child,\n\t\t}\n\t}\n\tpub const fn value(self) -> u32 {\n\t\tself.value\n\t}\n\tpub const fn origin(self) -> ProcessStatusOrigin {\n\t\tself.origin\n\t}\n}\n",
		"source": {
			"commit": "7d7d939",
			"path": "src/domain/src/execution.rs",
			"kind": "controlled-mutation"
		}
	},
	{
		"name": "comment-real-diagnostic-zero-sentinel",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "}\n\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct ProfileConfigurationError {\n\tpub file: String,\n\tpub line: usize,\n\tpub value: String,\n\tpub expected: &'static str,\n}\n\nimpl fmt::Display for ProfileConfigurationError {\n\tfn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {\n\t\twrite!(f, \"{}:{}: expected {}\", self.file, self.line, self.expected)\n\t}",
		"after": "}\n\n/// A canonical file diagnostic. Line zero identifies a missing key or input.\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct ProfileConfigurationError {\n\tpub file: String,\n\tpub line: usize,\n\tpub value: String,\n\tpub expected: &'static str,\n}\n\nimpl fmt::Display for ProfileConfigurationError {\n\tfn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {\n\t\twrite!(f, \"{}:{}: expected {}\", self.file, self.line, self.expected)\n\t}",
		"source": {
			"commit": "7d7d939",
			"path": "src/infrastructure/execution/src/profile.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-real-thread-affine-worker",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "\t\t\t\t\t#[cfg(windows)]\n\t\t\t\t\t{\n\t\t\t\t\t\tlet dispatcher = get_default(Clone::clone);\n\t\t\t\t\t\tlet span = Span::current();\n\t\t\t\t\t\tspawn_blocking(move || {\n\t\t\t\t\t\t\twith_default(&dispatcher, || {\n\t\t\t\t\t\t\t\tlet _entered = span.enter();\n\t\t\t\t\t\t\t\tlet runtime = Builder::new_current_thread()\n\t\t\t\t\t\t\t\t.enable_time()\n\t\t\t\t\t\t\t\t.build()\n\t\t\t\t\t\t\t\t.context(ErrorMarker::execution_supervision_failed())?;\n\t\t\t\t\t\t\t\truntime.block_on(adapter.execute(\n\t\t\t\t\t\t\t\t\toutput_target,\n\t\t\t\t\t\t\t\t\tworking_directory,",
		"after": "\t\t\t\t\t#[cfg(windows)]\n\t\t\t\t\t{\n\t\t\t\t\t\t// The upstream session is thread-affine. Its complete lifetime stays on\n\t\t\t\t\t\t// this blocking worker; only the owned result crosses back to Tokio.\n\t\t\t\t\t\tlet dispatcher = get_default(Clone::clone);\n\t\t\t\t\t\tlet span = Span::current();\n\t\t\t\t\t\tspawn_blocking(move || {\n\t\t\t\t\t\t\twith_default(&dispatcher, || {\n\t\t\t\t\t\t\t\tlet _entered = span.enter();\n\t\t\t\t\t\t\t\tlet runtime = Builder::new_current_thread()\n\t\t\t\t\t\t\t\t.enable_time()\n\t\t\t\t\t\t\t\t.build()\n\t\t\t\t\t\t\t\t.context(ErrorMarker::execution_supervision_failed())?;\n\t\t\t\t\t\t\t\truntime.block_on(adapter.execute(\n\t\t\t\t\t\t\t\t\toutput_target,\n\t\t\t\t\t\t\t\t\tworking_directory,",
		"source": {
			"commit": "7d7d939",
			"path": "src/infrastructure/dependencies/src/execution_adapter.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-real-utf16-command-line",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "impl Error for LaunchInputError {}\n\nfn encode_command_line(arguments: &[Vec<u16>]) -> Result<Vec<u16>, LaunchInputError> {\n\tlet mut output = Vec::new();\n\tfor (index, argument) in arguments.iter().enumerate() {\n\t\tif argument.contains(&0) {\n\t\t\treturn Err(report!(LaunchInputError::InvalidString));\n\t\t}\n\t\tif index != 0 {\n\t\t\toutput.push(32);\n\t\t}\n\t\toutput.push(34);\n\t\tlet mut backslashes = 0;\n\t\tfor &unit in argument {",
		"after": "impl Error for LaunchInputError {}\n\n// The upstream API takes an already encoded command line, not an argument vector.\n// Encode UTF-16 directly so unpaired Windows surrogates are never replaced.\nfn encode_command_line(arguments: &[Vec<u16>]) -> Result<Vec<u16>, LaunchInputError> {\n\tlet mut output = Vec::new();\n\tfor (index, argument) in arguments.iter().enumerate() {\n\t\tif argument.contains(&0) {\n\t\t\treturn Err(report!(LaunchInputError::InvalidString));\n\t\t}\n\t\tif index != 0 {\n\t\t\toutput.push(32);\n\t\t}\n\t\toutput.push(34);\n\t\tlet mut backslashes = 0;\n\t\tfor &unit in argument {",
		"source": {
			"commit": "7d7d939",
			"path": "src/infrastructure/execution/src/launch_inputs.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-real-native-open-safety",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "\n\t\tlet mut native = null_mut();\n\t\tlet result = unsafe { ffi::mods_usvfs_open(library.as_ptr(), instance.as_ptr(), &mut native) };\n\t\tif result.status != 0 {\n\t\t\tif result.cleanup_status == 0 {\n\t\t\t\tSESSION_ACTIVE.store(false, Ordering::Release);\n\t\t\t}\n\t\t\tcheck(result)?;\n\t\t}\n\t\tlet Some(native) = NonNull::new(native) else {\n\t\t\treturn Err(report!(ExecutionError));\n\t\t};\n\t\tOk(Self {\n\t\t\tnative: Some(native),",
		"after": "\n\t\tlet mut native = null_mut();\n\t\t// SAFETY: checked terminated strings remain live; output is writable. The\n\t\t// process-wide gate excludes concurrent upstream sessions. The shim catches\n\t\t// C++ exceptions and owns all resources on failure.\n\t\tlet result = unsafe { ffi::mods_usvfs_open(library.as_ptr(), instance.as_ptr(), &mut native) };\n\t\tif result.status != 0 {\n\t\t\tif result.cleanup_status == 0 {\n\t\t\t\tSESSION_ACTIVE.store(false, Ordering::Release);\n\t\t\t}\n\t\t\tcheck(result)?;\n\t\t}\n\t\tlet Some(native) = NonNull::new(native) else {\n\t\t\treturn Err(report!(ExecutionError));\n\t\t};\n\t\tOk(Self {\n\t\t\tnative: Some(native),",
		"source": {
			"commit": "4734903",
			"path": "src/infrastructure/execution/src/usvfs/mod.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-real-nonrecursive-priority-reason",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "\tpub(crate) fn apply(&self, native: &mut impl ConfigureView) -> Result<(), ExecutionError> {\n\t\tnative.clear_bypasses()?;\n\t\tnative.create_target(&self.data_target, false)?;\n\t\tfor mapping in &self.directories {\n\t\t\tnative.link_directory(mapping)?;\n\t\t}\n\t\tfor mapping in &self.files {\n\t\t\tnative.link_file(mapping)?;\n\t\t}\n\t\tnative.create_target(&self.saves, true)?;\n\t\tOk(())\n\t}\n}\n",
		"after": "\tpub(crate) fn apply(&self, native: &mut impl ConfigureView) -> Result<(), ExecutionError> {\n\t\tnative.clear_bypasses()?;\n\t\t// Nonrecursive: selecting a low-priority target must not remap its files last.\n\t\tnative.create_target(&self.data_target, false)?;\n\t\tfor mapping in &self.directories {\n\t\t\tnative.link_directory(mapping)?;\n\t\t}\n\t\tfor mapping in &self.files {\n\t\t\tnative.link_file(mapping)?;\n\t\t}\n\t\tnative.create_target(&self.saves, true)?;\n\t\tOk(())\n\t}\n}\n",
		"source": {
			"commit": "4734903",
			"path": "src/infrastructure/execution/src/configuration.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-real-fomod-not-semver",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "}\n\nfn parse_version(\n\tvalue: &str,\n\tbudget: &mut EvaluationBudget,\n\tcancellation: &CancellationToken,\n) -> Result<Vec<u32>, ErrorMarker> {\n\tlet mut parts = Vec::new();\n\tfor part in value.split('.') {\n\t\tif cancellation.is_cancelled() {\n\t\t\treturn Err(report!(ErrorMarker::operation_cancelled()));\n\t\t}\n\t\tbudget.spend()?;\n\t\tlet part = part",
		"after": "}\n\n// FOMOD versions are arbitrary-length numeric dot components, not SemVer. Evaluation also needs\n// cooperative cancellation and budget accounting, so a SemVer abstraction is unsuitable here.\nfn parse_version(\n\tvalue: &str,\n\tbudget: &mut EvaluationBudget,\n\tcancellation: &CancellationToken,\n) -> Result<Vec<u32>, ErrorMarker> {\n\tlet mut parts = Vec::new();\n\tfor part in value.split('.') {\n\t\tif cancellation.is_cancelled() {\n\t\t\treturn Err(report!(ErrorMarker::operation_cancelled()));\n\t\t}\n\t\tbudget.spend()?;\n\t\tlet part = part",
		"source": {
			"commit": "6daca92",
			"path": "application/src/installation/install_archive/fomod.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-real-write-all-control-errors",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "}\n\n#[derive(Debug)]\nstruct CallbackStopped;\n\nimpl Display for CallbackStopped {\n\tfn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {\n\t\tformatter.write_str(\"archive callback stopped\")\n\t}\n}\n\nimpl Error for CallbackStopped {}\n\n#[derive(Debug)]",
		"after": "}\n\n// Write::write_all retries Interrupted forever, so bridge control signals use distinct non-retryable errors.\n#[derive(Debug)]\nstruct CallbackStopped;\n\nimpl Display for CallbackStopped {\n\tfn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {\n\t\tformatter.write_str(\"archive callback stopped\")\n\t}\n}\n\nimpl Error for CallbackStopped {}\n\n#[derive(Debug)]",
		"source": {
			"commit": "6daca92",
			"path": "infrastructure/archive/src/extract.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-real-publication-cancellation-boundary",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": false,
		"before": "\t}\n\n\tvalidate_stage(root, LayoutLocation::Root, &CancellationToken::new()).context(publication_failed())?;\n\n\tdrop(stage);\n\tdrop(operation);\n\tcleanup_best_effort(root, temp, OPERATION_DIRECTORY);\n\tOk(())\n}\n\npub(crate) fn cleanup_best_effort(root: &SafeDir, temp: &SafeDir, operation_name: &str) {\n\tif temp.remove_dir_all(operation_name).is_ok() {\n\t\tlet _ = temp.sync();\n\t\tlet _ = root.sync();",
		"after": "\t}\n\n\t// mods.toml is the last required canonical mutation. Caller cancellation is no longer\n\t// observable once that durable rename succeeds.\n\tvalidate_stage(root, LayoutLocation::Root, &CancellationToken::new()).context(publication_failed())?;\n\n\tdrop(stage);\n\tdrop(operation);\n\tcleanup_best_effort(root, temp, OPERATION_DIRECTORY);\n\tOk(())\n}\n\npub(crate) fn cleanup_best_effort(root: &SafeDir, temp: &SafeDir, operation_name: &str) {\n\tif temp.remove_dir_all(operation_name).is_ok() {\n\t\tlet _ = temp.sync();\n\t\tlet _ = root.sync();",
		"source": {
			"commit": "6daca92",
			"path": "infrastructure/environment/src/publication.rs",
			"kind": "excerpt"
		}
	},
	{
		"name": "comment-mutated-thread-affine-worker",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": true,
		"before": "\t\t\t\t\t#[cfg(windows)]\n\t\t\t\t\t{\n\t\t\t\t\t\tlet dispatcher = get_default(Clone::clone);\n\t\t\t\t\t\tlet span = Span::current();\n\t\t\t\t\t\tspawn_blocking(move || {\n\t\t\t\t\t\t\twith_default(&dispatcher, || {\n\t\t\t\t\t\t\t\tlet _entered = span.enter();\n\t\t\t\t\t\t\t\tlet runtime = Builder::new_current_thread()\n\t\t\t\t\t\t\t\t.enable_time()\n\t\t\t\t\t\t\t\t.build()\n\t\t\t\t\t\t\t\t.context(ErrorMarker::execution_supervision_failed())?;\n\t\t\t\t\t\t\t\truntime.block_on(adapter.execute(\n\t\t\t\t\t\t\t\t\toutput_target,\n\t\t\t\t\t\t\t\t\tworking_directory,",
		"after": "\t\t\t\t\t#[cfg(windows)]\n\t\t\t\t\t{\n\t\t\t\t\t\t// Spawn a blocking task.\n\t\t\t\t\t\tlet dispatcher = get_default(Clone::clone);\n\t\t\t\t\t\tlet span = Span::current();\n\t\t\t\t\t\tspawn_blocking(move || {\n\t\t\t\t\t\t\twith_default(&dispatcher, || {\n\t\t\t\t\t\t\t\tlet _entered = span.enter();\n\t\t\t\t\t\t\t\tlet runtime = Builder::new_current_thread()\n\t\t\t\t\t\t\t\t.enable_time()\n\t\t\t\t\t\t\t\t.build()\n\t\t\t\t\t\t\t\t.context(ErrorMarker::execution_supervision_failed())?;\n\t\t\t\t\t\t\t\truntime.block_on(adapter.execute(\n\t\t\t\t\t\t\t\t\toutput_target,\n\t\t\t\t\t\t\t\t\tworking_directory,",
		"source": {
			"commit": "7d7d939",
			"path": "src/infrastructure/dependencies/src/execution_adapter.rs",
			"kind": "controlled-mutation"
		}
	},
	{
		"name": "comment-mutated-utf16-command-line",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": true,
		"before": "impl Error for LaunchInputError {}\n\nfn encode_command_line(arguments: &[Vec<u16>]) -> Result<Vec<u16>, LaunchInputError> {\n\tlet mut output = Vec::new();\n\tfor (index, argument) in arguments.iter().enumerate() {\n\t\tif argument.contains(&0) {\n\t\t\treturn Err(report!(LaunchInputError::InvalidString));\n\t\t}\n\t\tif index != 0 {\n\t\t\toutput.push(32);\n\t\t}\n\t\toutput.push(34);\n\t\tlet mut backslashes = 0;\n\t\tfor &unit in argument {",
		"after": "impl Error for LaunchInputError {}\n\n// Encode the command line.\nfn encode_command_line(arguments: &[Vec<u16>]) -> Result<Vec<u16>, LaunchInputError> {\n\tlet mut output = Vec::new();\n\tfor (index, argument) in arguments.iter().enumerate() {\n\t\tif argument.contains(&0) {\n\t\t\treturn Err(report!(LaunchInputError::InvalidString));\n\t\t}\n\t\tif index != 0 {\n\t\t\toutput.push(32);\n\t\t}\n\t\toutput.push(34);\n\t\tlet mut backslashes = 0;\n\t\tfor &unit in argument {",
		"source": {
			"commit": "7d7d939",
			"path": "src/infrastructure/execution/src/launch_inputs.rs",
			"kind": "controlled-mutation"
		}
	},
	{
		"name": "comment-mutated-nonrecursive-priority-reason",
		"ruleId": "comments-and-documentation-reason-comments",
		"expectedViolation": true,
		"before": "\tpub(crate) fn apply(&self, native: &mut impl ConfigureView) -> Result<(), ExecutionError> {\n\t\tnative.clear_bypasses()?;\n\t\tnative.create_target(&self.data_target, false)?;\n\t\tfor mapping in &self.directories {\n\t\t\tnative.link_directory(mapping)?;\n\t\t}\n\t\tfor mapping in &self.files {\n\t\t\tnative.link_file(mapping)?;\n\t\t}\n\t\tnative.create_target(&self.saves, true)?;\n\t\tOk(())\n\t}\n}\n",
		"after": "\tpub(crate) fn apply(&self, native: &mut impl ConfigureView) -> Result<(), ExecutionError> {\n\t\tnative.clear_bypasses()?;\n\t\t// Create the data target.\n\t\tnative.create_target(&self.data_target, false)?;\n\t\tfor mapping in &self.directories {\n\t\t\tnative.link_directory(mapping)?;\n\t\t}\n\t\tfor mapping in &self.files {\n\t\t\tnative.link_file(mapping)?;\n\t\t}\n\t\tnative.create_target(&self.saves, true)?;\n\t\tOk(())\n\t}\n}\n",
		"source": {
			"commit": "4734903",
			"path": "src/infrastructure/execution/src/configuration.rs",
			"kind": "controlled-mutation"
		}
	}
];
