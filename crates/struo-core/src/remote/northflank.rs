mod client;
mod execution;
mod ownership;
mod transport;

pub(super) use execution::execute_command;

#[cfg(test)]
mod tests;
