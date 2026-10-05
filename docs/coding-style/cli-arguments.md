# Coding style: CLI arguments

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

## CLI arguments

### Required positional arguments and optional named arguments

#### Applies to

- `src/presentation/**`

#### Rule

Use positional arguments for required CLI inputs. Use long options for optional inputs: `--argument <value>` for values and `--flag` for boolean switches. Inputs that callers may omit because they have defaults are optional and use long options. Do not make named options required or define optional positional arguments.

#### Violation

A command requires a named option, accepts an optional positional argument, or exposes an optional input only through a short option.

#### Compliant

Required inputs are positional. Optional inputs have long option names and may be omitted.

#### Bad example

For a command with a required source and an optional destination:

```text
tool copy --source <source> [<destination>]
```

#### Good example

```text
tool copy <source> [--destination <destination>]
```
