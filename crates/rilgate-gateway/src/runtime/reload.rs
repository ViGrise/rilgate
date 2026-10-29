use async_trait::async_trait;

pub enum ReloadOutcome {
  Reloaded,
  Unchanged,
  RestartRequired { sections: Vec<&'static str> },
}

/// Reloads the policy from the gateway's configuration sources.
#[async_trait]
pub trait ReloadControl: Send + Sync {
  async fn reload(&self) -> anyhow::Result<ReloadOutcome>;
}
