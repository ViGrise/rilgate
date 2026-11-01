use std::{
  io::{BufRead, BufReader, Read, Write},
  net::{TcpListener, TcpStream},
  path::Path,
  process::{Child, Command, Stdio},
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

const SECRET: &str = "reload-test-secret-with-at-least-32-bytes";
const TIMEOUT: Duration = Duration::from_secs(60);

fn free_port() -> u16 {
  TcpListener::bind("127.0.0.1:0")
    .unwrap()
    .local_addr()
    .unwrap()
    .port()
}

/// Answers every request with 200 until the test process ends.
fn upstream() -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let port = listener.local_addr().unwrap().port();
  std::thread::spawn(move || {
    for stream in listener.incoming().flatten() {
      std::thread::spawn(move || {
        let mut stream = stream;
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer);
        let _ =
          stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
      });
    }
  });
  port
}

fn configuration(listen: u16, upstream: u16, rules: &str) -> String {
  format!(
    "server:\n  listen: \"127.0.0.1:{listen}\"\n  upstream:\n    url: http://127.0.0.1:{upstream}\n\
     security:\n  secret: \"{SECRET}\"\n\
     policy:\n  default_action: allow\n  rules:\n{rules}"
  )
}

const DENY_BAD_BOT: &str =
  "    - name: bad-bot\n      when: {user_agent: {contains: BadBot}}\n      action: deny\n";
const ALLOW_ALL: &str =
  "    - name: everyone\n      when: {path: {prefix: /}}\n      action: allow\n";

struct Gateway {
  child: Child,
  logs: Arc<Mutex<Vec<String>>>,
  port: u16,
}

impl Gateway {
  fn start(file: &Path, port: u16) -> Self {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rilgate"))
      .current_dir(file.parent().unwrap())
      .env_clear()
      .args(["--config", file.to_str().unwrap(), "--log-format", "json"])
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    for stream in [
      Box::new(stdout) as Box<dyn Read + Send>,
      Box::new(stderr) as Box<dyn Read + Send>,
    ] {
      let logs = logs.clone();
      std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
          logs.lock().unwrap().push(line);
        }
      });
    }
    Self { child, logs, port }
  }

  fn wait_for(&self, text: &str, count: usize) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
      let found = self
        .logs
        .lock()
        .unwrap()
        .iter()
        .filter(|line| line.contains(text))
        .count();
      if found >= count {
        return;
      }
      assert!(
        Instant::now() < deadline,
        "no log line with {text:?}; logs:\n{}",
        self.logs.lock().unwrap().join("\n")
      );
      std::thread::sleep(Duration::from_millis(50));
    }
  }

  #[cfg(unix)]
  fn line(&self, text: &str) -> String {
    self
      .logs
      .lock()
      .unwrap()
      .iter()
      .rev()
      .find(|line| line.contains(text))
      .cloned()
      .unwrap_or_default()
  }

  #[cfg(unix)]
  fn hup(&self) {
    // SAFETY: the pid belongs to the child this value owns.
    let result = unsafe { libc::kill(self.child.id() as i32, libc::SIGHUP) };
    assert_eq!(result, 0);
  }

  fn status(&self, user_agent: &str) -> u16 {
    let deadline = Instant::now() + TIMEOUT;
    loop {
      if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", self.port)) {
        let request = format!(
          "GET /page HTTP/1.1\r\nhost: reload.test\r\nuser-agent: {user_agent}\r\nconnection: close\r\n\r\n"
        );
        let mut response = String::new();
        if stream.write_all(request.as_bytes()).is_ok()
          && stream.read_to_string(&mut response).is_ok()
          && let Some(status) = response.split_whitespace().nth(1)
        {
          return status.parse().unwrap();
        }
      }
      assert!(Instant::now() < deadline, "gateway did not answer");
      std::thread::sleep(Duration::from_millis(50));
    }
  }
}

impl Drop for Gateway {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

#[cfg(unix)]
#[test]
fn sighup_swaps_the_policy_and_keeps_the_running_state_on_other_changes_and_errors() {
  let directory = tempfile::tempdir().unwrap();
  let file = directory.path().join("rilgate.yml");
  let (port, upstream) = (free_port(), upstream());
  std::fs::write(&file, configuration(port, upstream, ALLOW_ALL)).unwrap();
  let gateway = Gateway::start(&file, port);
  assert_eq!(gateway.status("BadBot"), 200);

  std::fs::write(&file, configuration(port, upstream, DENY_BAD_BOT)).unwrap();
  gateway.hup();
  gateway.wait_for("Policy reloaded", 1);
  assert_eq!(gateway.status("BadBot"), 403);
  assert_eq!(gateway.status("Mozilla"), 200);

  let moved = configuration(port, upstream + 1, DENY_BAD_BOT).replace(DENY_BAD_BOT, ALLOW_ALL);
  std::fs::write(&file, moved).unwrap();
  gateway.hup();
  gateway.wait_for("Configuration change needs a restart", 1);
  let line = gateway.line("Configuration change needs a restart");
  assert!(line.contains("server"), "{line}");
  assert!(!line.contains("policy"), "{line}");
  assert_eq!(gateway.status("BadBot"), 403);
  assert_eq!(gateway.status("Mozilla"), 200);

  std::fs::write(&file, "server: [unterminated").unwrap();
  gateway.hup();
  gateway.wait_for("\"level\":\"ERROR\"", 1);
  assert_eq!(gateway.status("BadBot"), 403);
  assert_eq!(gateway.status("Mozilla"), 200);

  std::fs::write(&file, configuration(port, upstream, ALLOW_ALL)).unwrap();
  gateway.hup();
  gateway.wait_for("Policy reloaded", 2);
  assert_eq!(gateway.status("BadBot"), 200);
}

struct ReloadFixture {
  directory: tempfile::TempDir,
  file: std::path::PathBuf,
  rules: std::path::PathBuf,
  operations: u16,
  gateway: Gateway,
}

impl ReloadFixture {
  fn start(reload: bool, watch: bool) -> Self {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("rilgate.yml");
    let rules = directory.path().join("rules.yml");
    let (port, operations) = (free_port(), free_port());
    let config = configuration(port, upstream(), ALLOW_ALL)
      .replace(ALLOW_ALL, "    !include ./rules.yml\n")
      .replace("  upstream:", &format!("  watch_config: {watch}\n  operations:\n    listen: '127.0.0.1:{operations}'\n    reload: {reload}\n  upstream:"));
    let config = if reload {
      config
    } else {
      config.replace("    reload: false\n", "")
    };
    std::fs::write(&rules, ALLOW_ALL).unwrap();
    std::fs::write(&file, config).unwrap();
    let gateway = Gateway::start(&file, port);
    assert_eq!(gateway.status("BadBot"), 200);
    Self {
      directory,
      file,
      rules,
      operations,
      gateway,
    }
  }

  fn command(&self) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rilgate"))
      .env_clear()
      .current_dir(self.directory.path())
      .args(["--config", self.file.to_str().unwrap(), "reload"])
      .output()
      .unwrap()
  }

  fn route(&self, method: &str, headers: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", self.operations)).unwrap();
    stream
      .set_read_timeout(Some(Duration::from_secs(10)))
      .unwrap();
    write!(stream, "{method} /.well-known/rilgate/reload HTTP/1.1\r\nhost: reload.test\r\nconnection: close\r\ncontent-length: 0\r\n{headers}\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
  }

  fn wait_status(&self, expected: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      if self.gateway.status("BadBot") == expected {
        return;
      }
      assert!(
        Instant::now() < deadline,
        "policy did not change: {}",
        self.gateway.logs.lock().unwrap().join("\n")
      );
      std::thread::sleep(Duration::from_millis(50));
    }
  }

  fn replace(&self, path: &Path, text: &str) {
    let temporary = self.directory.path().join("replacement.yml");
    std::fs::write(&temporary, text).unwrap();
    std::fs::rename(temporary, path).unwrap();
  }
}

#[test]
fn reload_command_applies_policy() {
  let fixture = ReloadFixture::start(true, false);
  std::fs::write(&fixture.rules, DENY_BAD_BOT).unwrap();
  let output = fixture.command();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert_eq!(
    String::from_utf8_lossy(&output.stdout).trim(),
    "Policy reloaded"
  );
  assert_eq!(fixture.gateway.status("BadBot"), 403);
  assert_eq!(fixture.gateway.status("Mozilla"), 200);
  let output = fixture.command();
  assert!(output.status.success());
  assert_eq!(
    String::from_utf8_lossy(&output.stdout).trim(),
    "Policy is unchanged"
  );
  let response = fixture.route("GET", "");
  assert!(response.starts_with("HTTP/1.1 405"), "{response}");
  assert!(response.to_ascii_lowercase().contains("allow: post"));
  std::fs::write(&fixture.rules, "[unterminated").unwrap();
  let response = fixture.route("POST", "");
  assert!(response.starts_with("HTTP/1.1 500"), "{response}");
  assert!(response.contains("\"status\":\"failed\""));
  assert_eq!(fixture.gateway.status("BadBot"), 403);
}

#[test]
fn reload_command_reports_restart_required() {
  let fixture = ReloadFixture::start(true, false);
  let config = std::fs::read_to_string(&fixture.file)
    .unwrap()
    .replace("  upstream:", "  max_connections: 123\n  upstream:");
  std::fs::write(&fixture.file, config).unwrap();
  std::fs::write(&fixture.rules, DENY_BAD_BOT).unwrap();
  let output = fixture.command();
  assert_eq!(output.status.code(), Some(1));
  assert!(
    String::from_utf8_lossy(&output.stderr)
      .contains("Configuration change needs a restart: server")
  );
  assert_eq!(fixture.gateway.status("BadBot"), 200);
}

#[test]
fn reload_route_is_off_by_default() {
  let fixture = ReloadFixture::start(false, false);
  assert!(fixture.route("POST", "").starts_with("HTTP/1.1 404"));
  let output = fixture.command();
  assert_eq!(output.status.code(), Some(1));
  assert!(
    String::from_utf8_lossy(&output.stderr)
      .contains("the gateway is not running at that address or server.operations.reload is off")
  );
}

#[test]
fn reload_route_rejects_browser_origin() {
  let fixture = ReloadFixture::start(true, false);
  std::fs::write(&fixture.rules, DENY_BAD_BOT).unwrap();
  assert!(
    fixture
      .route("POST", "Origin: https://example.com\r\n")
      .starts_with("HTTP/1.1 403")
  );
  assert_eq!(fixture.gateway.status("BadBot"), 200);
}

#[test]
fn watch_applies_edited_include() {
  let fixture = ReloadFixture::start(false, true);
  fixture.replace(&fixture.rules, DENY_BAD_BOT);
  fixture.wait_status(403);
  fixture.replace(&fixture.rules, "[unterminated");
  fixture.gateway.wait_for("Configuration reload failed", 1);
  assert_eq!(fixture.gateway.status("BadBot"), 403);
  fixture.replace(&fixture.rules, ALLOW_ALL);
  fixture.wait_status(200);
}

#[test]
fn watch_follows_new_include() {
  let fixture = ReloadFixture::start(false, true);
  let second = fixture.directory.path().join("second.yml");
  std::fs::write(&second, DENY_BAD_BOT).unwrap();
  let config = std::fs::read_to_string(&fixture.file).unwrap().replace(
    "    !include ./rules.yml",
    "    - !include ./rules.yml\n    - !include ./second.yml",
  );
  // The first matching rule decides the request.
  std::fs::write(&fixture.rules, "[]").unwrap();
  fixture.replace(&fixture.file, &config);
  fixture.wait_status(403);
  fixture.replace(&second, ALLOW_ALL);
  fixture.wait_status(200);
}

#[cfg(unix)]
#[test]
fn watch_follows_configmap_symlink_replacement() {
  use std::os::unix::fs::symlink;
  let fixture = ReloadFixture::start(false, true);
  let first = fixture.directory.path().join("version-one");
  let second = fixture.directory.path().join("version-two");
  for directory in [&first, &second] {
    std::fs::create_dir(directory).unwrap();
  }
  std::fs::write(first.join("rules.yml"), DENY_BAD_BOT).unwrap();
  std::fs::write(second.join("rules.yml"), ALLOW_ALL).unwrap();
  let data = fixture.directory.path().join("..data");
  symlink(&first, &data).unwrap();
  let replacement = fixture.directory.path().join("replacement.yml");
  symlink("..data/rules.yml", &replacement).unwrap();
  std::fs::rename(&replacement, &fixture.rules).unwrap();
  fixture.wait_status(403);
  symlink(&second, &replacement).unwrap();
  std::fs::rename(&replacement, &data).unwrap();
  fixture.wait_status(200);
}

#[cfg(unix)]
#[test]
fn signals_change_reloads_without_restart() {
  let directory = tempfile::tempdir().unwrap();
  let file = directory.path().join("rilgate.yml");
  let (port, upstream) = (free_port(), upstream());
  let rules =
    "    - name: local-network\n      when: {client_ip: {in_sets: [local]}}\n      action: deny\n";
  let source = |cidr: &str| {
    format!(
      "{}\nsignals:\n  ip_sets:\n    local: {{cidrs: ['{cidr}']}}\n",
      configuration(port, upstream, rules)
    )
  };
  std::fs::write(&file, source("192.0.2.0/24")).unwrap();
  let gateway = Gateway::start(&file, port);
  assert_eq!(gateway.status("Mozilla"), 200);
  std::fs::write(&file, source("127.0.0.0/8")).unwrap();
  gateway.hup();
  gateway.wait_for("Policy reloaded", 1);
  assert_eq!(gateway.status("Mozilla"), 403);
  assert!(
    !gateway
      .logs
      .lock()
      .unwrap()
      .iter()
      .any(|line| line.contains("Configuration change needs a restart"))
  );
}
