//! rmcp 3.3.0's EOF drain bypasses its cancelled-request pool. Keep cancelled
//! responses off the wire even when a supervised operation finishes in that drain.
//! rmcp exposes no writer commitment hook or split reader. The outgoing Sink
//! checks cancellation at start_send under the same state lock as receive.
//! AsyncRwTransport retains its exact parsing/error policy; its protocol errors
//! are relayed through the same Sink instead of a second stdout writer.

use crate::lifecycle::Lifecycle;
use futures_util::Sink;
use futures_util::SinkExt;
use futures_util::StreamExt;
use rmcp::RoleServer;
use rmcp::model::ClientNotification;
use rmcp::model::ClientRequest;
use rmcp::model::GetMeta;
use rmcp::model::JsonRpcMessage;
use rmcp::model::ProgressToken;
use rmcp::model::RequestId;
use rmcp::model::ServerNotification;
use rmcp::service::RxJsonRpcMessage;
use rmcp::service::TxJsonRpcMessage;
use rmcp::transport::Transport;
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::transport::async_rw::JsonRpcMessageCodec;
use rmcp::transport::async_rw::JsonRpcMessageCodecError;
use std::collections::HashMap;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::DuplexStream;
use tokio::io::duplex;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tokio_util::codec::FramedRead;
use tokio_util::codec::FramedWrite;

pub(crate) struct CancellationTransport<T> {
	inner: T,
	lifecycle: Option<Arc<Lifecycle>>,
	state: Arc<Mutex<CancellationState>>,
}

impl<T> CancellationTransport<T> {
	#[cfg(test)]
	pub(crate) fn new(inner: T) -> Self {
		Self {
			inner,
			lifecycle: None,
			state: Arc::default(),
		}
	}
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for CancellationTransport<T> {
	type Error = T::Error;

	fn send(
		&mut self,
		message: TxJsonRpcMessage<RoleServer>,
	) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
		let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		let id = match &message {
			JsonRpcMessage::Response(response) => Some(&response.id),
			JsonRpcMessage::Error(error) => error.id.as_ref(),
			JsonRpcMessage::Notification(notification) => match &notification.notification {
				ServerNotification::ProgressNotification(progress) => {
					state.progress.get(&progress.params.progress_token)
				}
				_ => None,
			},
			_ => None,
		};
		let suppressed = id.is_some_and(|id| state.cancelled.contains(id))
			|| self.lifecycle
				.as_ref()
				.is_some_and(|lifecycle| lifecycle.shutdown.is_cancelled());
		drop(state);

		let response_id = match &message {
			JsonRpcMessage::Response(response) => Some(response.id.clone()),
			JsonRpcMessage::Error(error) => error.id.clone(),
			_ => None,
		};
		let lifecycle = self.lifecycle.clone();

		let send = if suppressed {
			None
		} else {
			Some(self.inner.send(message))
		};

		async move {
			let result = if let Some(send) = send { send.await } else { Ok(()) };
			if let Some(lifecycle) = lifecycle {
				if result.is_err() {
					lifecycle.shutdown.cancel();
				}
				if let Some(id) = response_id {
					lifecycle.release_response(&id).await;
				}
			}

			result
		}
	}

	async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
		let Some(message) = self.inner.receive().await else {
			if let Some(lifecycle) = &self.lifecycle {
				lifecycle.shutdown.cancel();
			}
			return None;
		};

		let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		match &message {
			JsonRpcMessage::Request(request) => {
				state.cancelled.remove(&request.id);
				state.progress.retain(|_, id| id != &request.id);
				if matches!(&request.request, ClientRequest::CallToolRequest(_))
					&& let Some(token) = request.request.get_meta().get_progress_token()
				{
					state.progress.insert(token, request.id.clone());
				}
			}
			JsonRpcMessage::Notification(notification) => {
				if let ClientNotification::CancelledNotification(cancelled) = &notification.notification
					&& let Some(id) = &cancelled.params.request_id
				{
					state.cancelled.insert(id.clone());
				}
			}
			_ => {}
		}

		Some(message)
	}

	async fn close(&mut self) -> Result<(), Self::Error> {
		if let Some(lifecycle) = &self.lifecycle {
			lifecycle.shutdown.cancel();
		}

		self.inner.close().await
	}
}

#[derive(Default)]
struct CancellationState {
	cancelled: HashSet<RequestId>,
	progress: HashMap<ProgressToken, RequestId>,
}

struct CommitmentSink<S> {
	inner: S,
	state: Arc<Mutex<CancellationState>>,
	lifecycle: Arc<Lifecycle>,
}

impl<S: Sink<TxJsonRpcMessage<RoleServer>> + Unpin> Sink<TxJsonRpcMessage<RoleServer>> for CommitmentSink<S> {
	type Error = S::Error;

	fn poll_ready(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.inner).poll_ready(context)
	}

	fn start_send(self: Pin<&mut Self>, message: TxJsonRpcMessage<RoleServer>) -> Result<(), Self::Error> {
		let this = self.get_mut();
		let state = this.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		let id = match &message {
			JsonRpcMessage::Response(response) => Some(&response.id),
			JsonRpcMessage::Error(error) => error.id.as_ref(),
			JsonRpcMessage::Notification(notification) => match &notification.notification {
				ServerNotification::ProgressNotification(progress) => {
					state.progress.get(&progress.params.progress_token)
				}
				_ => None,
			},
			_ => None,
		};
		if this.lifecycle.shutdown.is_cancelled() || id.is_some_and(|id| state.cancelled.contains(id)) {
			return Ok(());
		}

		// Only synchronous encoding is inside this lock. Once committed, flush is
		// allowed to finish even if cancellation arrives while stdout is blocked.
		Pin::new(&mut this.inner).start_send(message)
	}

	fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.inner).poll_flush(context)
	}

	fn poll_close(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut self.inner).poll_close(context)
	}
}

type SerializedWriter<W> =
	Arc<AsyncMutex<CommitmentSink<FramedWrite<W, JsonRpcMessageCodec<TxJsonRpcMessage<RoleServer>>>>>>;

pub(crate) struct IoTransport<R: AsyncRead, W> {
	reader: AsyncRwTransport<RoleServer, R, DuplexStream>,
	writer: SerializedWriter<W>,
	relay: Option<JoinHandle<()>>,
}

pub(crate) fn io_transport<R, W>(
	reader: R,
	writer: W,
	lifecycle: Arc<Lifecycle>,
) -> CancellationTransport<IoTransport<R, W>>
where
	R: AsyncRead + Send + Unpin + 'static,
	W: AsyncWrite + Send + Unpin + 'static,
{
	let state = Arc::default();
	let writer = Arc::new(AsyncMutex::new(CommitmentSink {
		inner: FramedWrite::new(writer, JsonRpcMessageCodec::new()),
		state: Arc::clone(&state),
		lifecycle: lifecycle.clone(),
	}));
	let (errors, relay) = duplex(4096);
	let relay_writer = writer.clone();
	let shutdown = lifecycle.shutdown.clone();

	let relay = lifecycle.tasks.spawn(async move {
		let mut relay = FramedRead::new(relay, JsonRpcMessageCodec::<TxJsonRpcMessage<RoleServer>>::new());
		loop {
			let sent = async {
				let message = relay.next().await?;
				Some(match message {
					Ok(message) => relay_writer.lock().await.send(message).await,
					Err(error) => Err(error),
				})
			};
			tokio::select! {
				() = shutdown.cancelled() => break,
				result = sent => match result {
					Some(Ok(())) => {},
					Some(Err(_)) => { shutdown.cancel(); break; },
					None => break,
				},
			}
		}
	});

	CancellationTransport {
		inner: IoTransport {
			reader: AsyncRwTransport::new_server(reader, errors),
			writer,
			relay: Some(relay),
		},
		lifecycle: Some(lifecycle),
		state,
	}
}

impl<R, W> Transport<RoleServer> for IoTransport<R, W>
where
	R: AsyncRead + Send + Unpin,
	W: AsyncWrite + Send + Unpin + 'static,
{
	type Error = JsonRpcMessageCodecError;

	fn send(
		&mut self,
		message: TxJsonRpcMessage<RoleServer>,
	) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
		let writer = self.writer.clone();
		async move { writer.lock().await.send(message).await }
	}

	async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
		let message = self.reader.receive().await;
		if message.is_some() {
			return message;
		}

		// The SDK has already sent these errors before observing EOF. Drain the
		// private stream before outer EOF handling cancels uncommitted output.
		let _ = self.reader.close().await;
		if let Some(relay) = &mut self.relay {
			let _ = relay.await;
		}
		self.relay = None;

		None
	}

	async fn close(&mut self) -> Result<(), Self::Error> {
		self.reader.close().await?;
		self.writer.lock().await.close().await
	}
}

#[cfg(test)]
mod tests {
	use super::CancellationTransport;
	use super::io_transport;
	use crate::lifecycle::Lifecycle;
	use futures_util::poll;
	use rmcp::ErrorData;
	use rmcp::RoleServer;
	use rmcp::ServerHandler;
	use rmcp::ServiceExt;
	use rmcp::model::CallToolRequestParams;
	use rmcp::model::CallToolResponse;
	use rmcp::model::CallToolResult;
	use rmcp::service::RequestContext;
	use rmcp::service::RxJsonRpcMessage;
	use rmcp::service::TxJsonRpcMessage;
	use rmcp::transport::Transport;
	use serde_json::Value;
	use serde_json::from_str;
	use serde_json::from_value;
	use serde_json::json;
	use serde_json::to_value;
	use std::convert::Infallible;
	use std::error::Error;
	use std::future::ready;
	use std::pin::pin;
	use std::sync::Arc;
	use std::task::Poll;
	use tokio::io::AsyncBufReadExt;
	use tokio::io::AsyncWriteExt;
	use tokio::io::BufReader;
	use tokio::io::duplex;
	use tokio::spawn;
	use tokio::sync::Notify;
	use tokio::sync::mpsc;

	#[tokio::test]
	async fn queued_progress_is_dropped_but_committed_response_finishes() -> Result<(), Box<dyn Error + Send + Sync>>
	{
		let (mut input, reader) = duplex(4096);
		let (writer, output) = duplex(1);
		let mut output = BufReader::new(output);
		let lifecycle = Arc::new(Lifecycle::default());
		let mut transport = io_transport(reader, writer, lifecycle.clone());

		input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"mods_exec\",\"_meta\":{\"progressToken\":\"p\"}}}\n").await?;
		assert!(transport.receive().await.is_some());

		let mut committed = pin!(transport.send(from_value(json!({"jsonrpc":"2.0","id":2,"result":{}}))?));
		assert!(matches!(poll!(&mut committed), Poll::Pending));
		let mut queued = pin!(transport.send(from_value(
			json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"p","progress":1}})
		)?));
		assert!(matches!(poll!(&mut queued), Poll::Pending));
		let mut queued_response =
			pin!(transport.send(from_value(json!({"jsonrpc":"2.0","id":2,"result":{}}))?));
		assert!(matches!(poll!(&mut queued_response), Poll::Pending));
		input.write_all(
			b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":2}}\n",
		)
		.await?;
		assert!(transport.receive().await.is_some());

		let mut line = String::new();
		let (sent, read) = tokio::join!(&mut committed, output.read_line(&mut line));
		sent?;
		read?;
		assert!(line.contains("\"id\":2"));
		assert!(
			matches!(poll!(&mut queued), Poll::Ready(Ok(()))),
			"cancelled queued frame reached the writer"
		);
		assert!(
			matches!(poll!(&mut queued_response), Poll::Ready(Ok(()))),
			"cancelled queued response reached the writer"
		);

		transport.close().await?;
		lifecycle.drain().await;
		Ok(())
	}

	#[tokio::test]
	async fn reader_preserves_syntax_recovery_and_invalid_request_response()
	-> Result<(), Box<dyn Error + Send + Sync>> {
		let (mut input, reader) = duplex(4096);
		let (writer, output) = duplex(4096);
		let mut output = BufReader::new(output);
		let lifecycle = Arc::new(Lifecycle::default());
		let mut transport = io_transport(reader, writer, lifecycle.clone());

		input.write_all(b"not json\n{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":4}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n").await?;
		let received = transport.receive().await;
		assert_eq!(to_value(received)?["id"], 2);
		let mut line = String::new();
		output.read_line(&mut line).await?;
		let error: Value = from_str(&line)?;
		assert_eq!(error["error"]["code"], -32600);

		transport.close().await?;
		lifecycle.drain().await;
		Ok(())
	}

	#[tokio::test]
	async fn invalid_request_response_is_delivered_before_eof() -> Result<(), Box<dyn Error + Send + Sync>> {
		let (mut input, reader) = duplex(4096);
		let (writer, output) = duplex(4096);
		let mut output = BufReader::new(output);
		let lifecycle = Arc::new(Lifecycle::default());
		let mut transport = io_transport(reader, writer, lifecycle.clone());

		input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":4}\n")
			.await?;
		input.shutdown().await?;
		assert!(transport.receive().await.is_none());

		transport.close().await?;
		lifecycle.drain().await;

		let mut line = String::new();
		output.read_line(&mut line).await?;
		assert!(!line.is_empty(), "invalid-request response was lost at EOF");
		let error: Value = from_str(&line)?;
		assert_eq!(error["error"]["code"], -32600);
		assert!(error["id"].is_null());

		Ok(())
	}

	struct ShutdownTransport {
		incoming: mpsc::UnboundedReceiver<RxJsonRpcMessage<RoleServer>>,
		outgoing: mpsc::UnboundedSender<TxJsonRpcMessage<RoleServer>>,
		eof: Arc<Notify>,
	}

	impl Transport<RoleServer> for ShutdownTransport {
		type Error = Infallible;

		fn send(
			&mut self,
			message: TxJsonRpcMessage<RoleServer>,
		) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
			let _ = self.outgoing.send(message);
			ready(Ok(()))
		}

		async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
			let message = self.incoming.recv().await;
			if message.is_none() {
				self.eof.notify_one();
			}
			message
		}

		async fn close(&mut self) -> Result<(), Self::Error> {
			Ok(())
		}
	}

	struct FinishingOperation {
		cancelled: Arc<Notify>,
		eof: Arc<Notify>,
	}

	impl ServerHandler for FinishingOperation {
		async fn call_tool(
			&self,
			_: CallToolRequestParams,
			context: RequestContext<RoleServer>,
		) -> Result<CallToolResponse, ErrorData> {
			context.ct.cancelled().await;
			self.cancelled.notify_one();
			self.eof.notified().await;
			Ok(CallToolResult::success(Vec::new()).into())
		}
	}

	#[tokio::test]
	async fn cancellation_suppresses_response_while_transport_drains() -> Result<(), Box<dyn Error + Send + Sync>> {
		let (input, incoming) = mpsc::unbounded_channel();
		let (outgoing, mut output) = mpsc::unbounded_channel();
		let eof = Arc::new(Notify::new());
		let cancelled = Arc::new(Notify::new());
		let transport = ShutdownTransport {
			incoming,
			outgoing,
			eof: eof.clone(),
		};
		let operation = FinishingOperation {
			cancelled: cancelled.clone(),
			eof,
		};
		let server = spawn(async move {
			operation
				.serve(CancellationTransport::new(transport))
				.await?
				.waiting()
				.await?;
			Ok::<(), Box<dyn Error + Send + Sync>>(())
		});

		input.send(from_value(json!({
			"jsonrpc": "2.0", "id": 1, "method": "initialize",
			"params": { "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }
		}))?)?;
		assert!(output.recv().await.is_some());
		input.send(from_value(
			json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
		)?)?;
		input.send(from_value(
			json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "mods_exec", "arguments": {}}}),
		)?)?;
		input.send(from_value(
			json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 2}}),
		)?)?;
		cancelled.notified().await;
		drop(input);

		server.await??;
		assert!(
			output.recv().await.is_none(),
			"cancelled response reached the wire during shutdown"
		);
		Ok(())
	}
	#[tokio::test]
	async fn cancellation_suppresses_late_progress_for_only_the_cancelled_token()
	-> Result<(), Box<dyn Error + Send + Sync>> {
		let (input, incoming) = mpsc::unbounded_channel();
		let (outgoing, mut output) = mpsc::unbounded_channel();
		let mut transport = CancellationTransport::new(ShutdownTransport {
			incoming,
			outgoing,
			eof: Arc::new(Notify::new()),
		});
		input.send(from_value(json!({
			"jsonrpc": "2.0", "id": 2, "method": "tools/call",
			"params": {"name": "mods_exec", "arguments": {}, "_meta": {"progressToken": "cancelled"}}
		}))?)?;
		transport.receive().await;
		input.send(from_value(json!({
			"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 2}
		}))?)?;
		transport.receive().await;

		for token in ["cancelled", "active"] {
			transport
				.send(from_value(json!({
					"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": token, "progress": 1}
				}))?)
				.await?;
		}

		let delivered = to_value(output.try_recv()?)?;
		assert_eq!(delivered.pointer("/params/progressToken"), Some(&json!("active")));
		assert!(output.try_recv().is_err());
		Ok(())
	}
}
