# CLI problem types

Each `type` points to a permanent anchor below. The final path fragment is also the convenience `code` field. Read [JSON output](json.md) for the error contract.

<a id="invalid_arguments"></a>
## invalid_arguments

The command line is malformed or uses an unknown command, option, or value.

<a id="environment_root_selection_failed"></a>
## environment_root_selection_failed

The CLI cannot select a usable Environment Root before dependency setup.

<a id="startup_failed"></a>
## startup_failed

The CLI could not determine the caller startup directory.

<a id="operation_failed"></a>
## operation_failed

An operation failed without a safe application error marker.

<a id="nexus_file_selection_required"></a>
## nexus_file_selection_required

A Nexus mod-page input has zero or several available Main files, so the CLI cannot choose one. `exit_code` is 2. `details.files` lists every available file in the published order, each with `file_id`, `name`, `version`, and `category`. Rerun with `--file ID`.

<a id="nexus_source_invalid"></a>
## nexus_source_invalid

The Nexus source or file selection is invalid. Use a New Vegas Nexus mod-page or file URL. Do not combine `--file` with a local archive path or with a different URL file ID.

<a id="nexus_premium_required"></a>
## nexus_premium_required

A new Nexus download needs a Premium account API key in `nexus_api_key` or `MODS_NEXUS_API_KEY`.

<a id="nexus_credentials_invalid"></a>
## nexus_credentials_invalid

Nexus rejected the configured API key.

<a id="nexus_access_denied"></a>
## nexus_access_denied

Nexus denied access to the mod or file.

<a id="nexus_rate_limited"></a>
## nexus_rate_limited

Nexus rate limited the request.

<a id="nexus_unavailable"></a>
## nexus_unavailable

The Nexus mod or selected file is missing, removed, or not downloadable.

<a id="nexus_network_failure"></a>
## nexus_network_failure

A Nexus request or transfer failed. Partial download bytes are removed.

<a id="nexus_response_invalid"></a>
## nexus_response_invalid

Nexus returned a response that the CLI cannot use.

<a id="environment_not_initialized"></a>
## environment_not_initialized

The Environment Root has not been initialized.

<a id="environment_already_initialized"></a>
## environment_already_initialized

The Environment Root has already been initialized.

<a id="environment_root_not_empty"></a>
## environment_root_not_empty

Initialization found files in the target Environment Root.

<a id="environment_root_unsafe"></a>
## environment_root_unsafe

The target Environment Root is unsafe.

<a id="environment_schema_unsupported"></a>
## environment_schema_unsupported

The Environment Manifest uses an unsupported schema.

<a id="environment_invalid"></a>
## environment_invalid

The selected Mod Environment is invalid. `details.phase` can name the invalid part. For example, `download_cache` means the Nexus download cache is invalid.

<a id="environment_publication_failed"></a>
## environment_publication_failed

Publishing an initialized Mod Environment failed.

<a id="manual_cleanup_required"></a>
## manual_cleanup_required

An unfinished operation needs manual cleanup.

<a id="game_install_not_found"></a>
## game_install_not_found

The requested Game Installation was not found.

<a id="game_install_invalid"></a>
## game_install_invalid

The Game Installation cannot be used.

<a id="game_build_mismatch"></a>
## game_build_mismatch

The observed Game Installation build differs from the recorded build.

<a id="setting_unknown"></a>
## setting_unknown

The requested setting is unknown.

<a id="setting_read_only"></a>
## setting_read_only

The requested setting cannot be changed.

<a id="setting_value_invalid"></a>
## setting_value_invalid

The supplied setting value is invalid.

<a id="invalid_selection"></a>
## invalid_selection

A supplied FOMOD Choice is invalid. `details` may identify a field, group, option, or sequence.

<a id="unsupported_installer"></a>
## unsupported_installer

The archive uses an unsupported installer.

<a id="dependency_unsatisfied"></a>
## dependency_unsatisfied

An installer dependency is not satisfied.

<a id="unsafe_archive"></a>
## unsafe_archive

The supplied archive is unsafe.

<a id="ambiguous_install_plan"></a>
## ambiguous_install_plan

The archive does not yield one safe Install Plan.

<a id="invalid_mod_name"></a>
## invalid_mod_name

The supplied Mod Name is invalid.

<a id="invalid_data_path"></a>
## invalid_data_path

The supplied Data-relative path is invalid.

<a id="mod_already_exists"></a>
## mod_already_exists

The target Data Mod exists and replacement was not requested.

<a id="mod_not_found"></a>
## mod_not_found

The requested Data Mod does not exist.

<a id="io_failure"></a>
## io_failure

An input or output operation failed.

<a id="transaction_failure"></a>
## transaction_failure

An installation transaction failed.

<a id="invalid_output_target"></a>
## invalid_output_target

The requested Output Target is invalid.

<a id="output_target_not_found"></a>
## output_target_not_found

The requested Output Target was not found.

<a id="output_target_disabled"></a>
## output_target_disabled

The requested Output Target is disabled.

<a id="invalid_working_directory"></a>
## invalid_working_directory

The requested working directory is invalid.

<a id="program_not_found"></a>
## program_not_found

The child program was not found before launch.

<a id="program_unsupported"></a>
## program_unsupported

The child program is unsupported before launch.

<a id="program_launch_failed"></a>
## program_launch_failed

Starting the child program failed.

<a id="vfs_failed"></a>
## vfs_failed

Setting up or cleaning up the Virtual Game View failed. `details.phase` identifies the safe reported phase when available.

<a id="execution_supervision_failed"></a>
## execution_supervision_failed

Managing the child process failed. A failure after launch is plain stderr, not this JSON type.

<a id="operation_cancelled"></a>
## operation_cancelled

The operation was cancelled.

<a id="shortcut_unsupported"></a>
## shortcut_unsupported

Launch Shortcuts are supported only on Windows.

<a id="shortcut_name_invalid"></a>
## shortcut_name_invalid

The shortcut name is not a valid Windows filename, or it contains a path.

<a id="shortcut_destination_invalid"></a>
## shortcut_destination_invalid

The destination is not a directory, or an existing entry with the shortcut name is not a Shell Link that may be replaced.

<a id="shortcut_launch_invalid"></a>
## shortcut_launch_invalid

The CLI could not capture valid absolute paths for the launcher, Environment Root, program, or working directory.

<a id="shortcut_arguments_too_long"></a>
## shortcut_arguments_too_long

The saved `mods exec --hidden` arguments exceed the Shell Link storage limit.

<a id="shortcut_failed"></a>
## shortcut_failed

Preparing, verifying, or publishing the Shell Link failed.
