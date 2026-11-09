/** Opens a socket to `url`. Close it with {@link disconnect} when you're done. */
export function openSocket(url: string): WebSocket {
  return new WebSocket(url);
}

/** Closes a socket that {@link openSocket} opened. */
export function closeSocket(socket: WebSocket): void {
  socket.close();
}
