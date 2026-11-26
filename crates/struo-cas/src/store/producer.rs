use super::{stream, Cas};
use crate::{Digest, Result};

impl Cas {
    #[cfg(test)]
    pub(crate) async fn put_produced<W, G>(&self, guard: G, produce: W) -> Result<Digest>
    where
        W: FnOnce(&mut dyn std::io::Write) -> std::io::Result<()> + Send + 'static,
        G: Send + 'static,
    {
        self.put_produced_sized(guard, None, produce).await
    }

    #[cfg(test)]
    pub(crate) async fn put_produced_pipelined<W, G>(&self, guard: G, produce: W) -> Result<Digest>
    where
        W: FnOnce(&mut dyn std::io::Write) -> std::io::Result<()> + Send + 'static,
        G: Send + 'static,
    {
        self.put_produced_pipelined_sized(guard, None, produce)
            .await
    }

    pub(crate) async fn put_produced_sized<W, G>(
        &self,
        guard: G,
        expected: Option<u64>,
        produce: W,
    ) -> Result<Digest>
    where
        W: FnOnce(&mut dyn std::io::Write) -> std::io::Result<()> + Send + 'static,
        G: Send + 'static,
    {
        stream::produce(self, guard, produce, false, expected).await
    }

    pub(crate) async fn put_produced_pipelined_sized<W, G>(
        &self,
        guard: G,
        expected: Option<u64>,
        produce: W,
    ) -> Result<Digest>
    where
        W: FnOnce(&mut dyn std::io::Write) -> std::io::Result<()> + Send + 'static,
        G: Send + 'static,
    {
        stream::produce(self, guard, produce, true, expected).await
    }
}

#[cfg(test)]
#[path = "producer_tests.rs"]
mod tests;
