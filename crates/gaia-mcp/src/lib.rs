//! rmcp の ServerHandler を gaia_core::tools::ToolService に接続する薄い層。
pub mod http;
pub mod server;
pub mod sources;
pub mod stdio;

pub use http::{BoundServer, HttpServeError, serve_http};
pub use server::GaiaServer;
pub use stdio::{ServeError, serve_stdio};
