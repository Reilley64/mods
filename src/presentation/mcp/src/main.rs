#![forbid(unsafe_code)]

#[expect(
	clippy::unimplemented,
	reason = "MCP presentation is implemented in its owning ticket"
)]
fn main() {
	unimplemented!();
}
