//! gaia-library のコア: 契約・ストレージ・ドメイン・ToolService。
//! MCP と CLI はこの crate の `tools::ToolService` だけを入口にする。

pub mod contracts;
pub mod error;
pub mod identity;
pub mod storage;
