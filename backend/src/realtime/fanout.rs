use crate::realtime::ConnectionRegistry;
use tokio_tungstenite::tungstenite::protocol::Message;
use crate::realtime::policy::handle_overflow;

pub async fn fanout_message(
    registry: &ConnectionRegistry,
    message: Message,
) -> Result<(), String> {
    let connections = registry.get_all().await;

    for (id, sender) in connections {
        match sender.send(message.clone()) {
            Ok(_) => {
                // Successfully sent to this connection
            }
            Err(_) => {
                // If the connection's queue is full, handle overflow
                let _ = handle_overflow(&id).await;
                registry.remove(&id).await;
            }
        }
    }

    Ok(())
}