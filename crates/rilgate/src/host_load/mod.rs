const HOST_LOAD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) async fn sample_host_load() {
  let mut interval = tokio::time::interval(HOST_LOAD_INTERVAL);
  let mut warned = false;
  loop {
    interval.tick().await;
    match rilgate_policy::read_host_load() {
      Some(averages) => rilgate_policy::HOST_LOAD.store(averages),
      None if !warned => {
        warned = true;
        tracing::warn!("host load average is unavailable, load conditions stay unknown");
      }
      None => {}
    }
  }
}
