//! One-request NDJSON transport over the shared cooperative execution control.
use kronello_service::{
    CLI_EVENT_VERSION, CliEvent, CliEventOutcome, ExecutionControl, Response, ServiceError,
};
use std::io::{Read, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub struct Stream {
    cancelled: Arc<AtomicBool>,
    output: Mutex<Output>,
}
struct Output {
    sequence: u64,
    failed: bool,
}
impl Stream {
    pub fn start() -> Result<Self, ServiceError> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        // Register synchronously before emitting the header or reading stdin.
        #[cfg(unix)]
        let mut signal = {
            let _guard = runtime.enter();
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?
        };
        #[cfg(windows)]
        let mut signal = {
            let _guard = runtime.enter();
            tokio::signal::windows::ctrl_c()?
        };
        let flag = cancelled.clone();
        std::thread::spawn(move || {
            runtime.block_on(async move {
                while signal.recv().await.is_some() {
                    flag.store(true, Ordering::SeqCst);
                }
            })
        });
        let stream = Self {
            cancelled,
            output: Mutex::new(Output {
                sequence: 0,
                failed: false,
            }),
        };
        stream.emit(|sequence| CliEvent::Header {
            version: CLI_EVENT_VERSION,
            sequence,
        });
        if stream.output.lock().expect("output lock").failed {
            return Err(ServiceError::new(
                "OUTPUT_IO_ERROR",
                "cannot write stream header",
            ));
        }
        Ok(stream)
    }
    fn emit(&self, record: impl FnOnce(u64) -> CliEvent) {
        let mut output = self.output.lock().expect("output lock");
        if output.failed {
            return;
        }
        let event = record(output.sequence);
        let result = serde_json::to_vec(&event)
            .map_err(std::io::Error::other)
            .and_then(|mut bytes| {
                bytes.push(b'\n');
                let mut stdout = std::io::stdout().lock();
                stdout.write_all(&bytes).and_then(|()| stdout.flush())
            });
        if let Err(error) = result {
            eprintln!("OUTPUT_IO_ERROR: {error}");
            output.failed = true;
            self.cancelled.store(true, Ordering::SeqCst);
        }
        output.sequence += 1;
    }
    pub fn read_request(&self) -> Result<String, ServiceError> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut json = String::new();
            let result = std::io::stdin()
                .take(16 * 1024 * 1024 + 1)
                .read_to_string(&mut json)
                .map(|_| json);
            let _ = sender.send(result);
        });
        loop {
            if self.is_cancelled() {
                return Err(ServiceError::new(
                    "REQUEST_CANCELLED",
                    "cancelled while reading request",
                ));
            }
            match receiver.recv_timeout(std::time::Duration::from_millis(20)) {
                Ok(result) => return result.map_err(Into::into),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(ServiceError::invalid("request reader disconnected")),
            }
        }
    }
    pub fn finish(&self, response: Response) -> bool {
        let outcome = match &response {
            Response::Success { .. } => CliEventOutcome::End,
            Response::Error { error } if error.code == "REQUEST_CANCELLED" => {
                CliEventOutcome::Cancelled
            }
            Response::Error { .. } => CliEventOutcome::Error,
        };
        let failed = matches!(&response, Response::Error { .. });
        self.emit(|sequence| CliEvent::Terminal {
            sequence,
            outcome,
            response: Box::new(response),
        });
        failed || self.output.lock().expect("output lock").failed
    }
}
impl ExecutionControl for Stream {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    fn progress(&self, completed: u64, total: u64) {
        self.emit(|sequence| CliEvent::Progress {
            sequence,
            completed,
            total,
        });
    }
}
