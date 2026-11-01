//! 桌面应用层：与 UI 无关的状态机、事件桥与设置。
//!
//! 命令行入口（`src/main.rs`）和 NSH 图形界面（`crates/desktop-ui`）共用这里的
//! [`app::App`]、[`bridge::EventBridge`] 与 [`state::Settings`]，保证两种运行方式
//! 看到完全一致的抓包状态。

#![allow(dead_code)]

pub mod app;
pub mod bridge;
pub mod state;
pub mod storage;
