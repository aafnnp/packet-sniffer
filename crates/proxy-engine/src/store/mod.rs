//! 请求数据存储。MVP 阶段使用进程内内存存储，后续可替换为 SQLite 实现。

mod memory;

pub use memory::MemoryStore;
