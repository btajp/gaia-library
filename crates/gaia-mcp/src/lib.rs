//! rmcp の ServerHandler を gaia_core::tools::ToolService に接続する薄い層。
pub mod server;
pub mod stdio;

pub use server::GaiaServer;
pub use stdio::{ServeError, serve_stdio};
