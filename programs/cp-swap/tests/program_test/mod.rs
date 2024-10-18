pub mod program_test_bench;
pub mod cp_swap_test;
pub mod tools;
pub mod utils;
pub mod helpers;
use std::{sync::Arc, sync::RwLock};
use log::*;

#[derive(Default, Clone)]
pub struct ProgramOutput {
    pub logs: Vec<String>,
    pub data: Vec<String>,
}
struct LoggerWrapper {
    inner: env_logger::Logger,
    output: Arc<RwLock<ProgramOutput>>,
}

impl Log for LoggerWrapper {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if record
            .target()
            .starts_with("solana_runtime::message_processor")
        {
            let msg = record.args().to_string();
            if let Some(data) = msg.strip_prefix("Program log: ") {
                self.output.write().unwrap().logs.push(data.into());
            } else if let Some(data) = msg.strip_prefix("Program data: ") {
                self.output.write().unwrap().data.push(data.into());
            }
        }
        self.inner.log(record);
    }

    fn flush(&self) {}
}
