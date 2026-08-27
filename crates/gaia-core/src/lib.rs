//! gaia-library のコア: 契約・ストレージ・ドメイン・ToolService。
//! MCP と CLI はこの crate の `tools::ToolService` だけを入口にする。

pub mod admin;
pub mod config;
pub mod contracts;
pub mod domain;
pub mod error;
pub mod identity;
pub mod scope;
pub mod storage;
pub mod tools;
