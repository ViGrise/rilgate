#![cfg(unix)]

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
     policy:\n  mode: custom\n  default_action: allow\n  rules:\n{rules}"
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
