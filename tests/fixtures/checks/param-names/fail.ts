/**
 * Posts a message to a chat channel.
 *
 * @param channelId - The channel to post in.
 * @param text - What the message says.
 */
export async function post(channel: string, text: string): Promise<void> {
  await fetch(`/api/channels/${channel}/messages`, { method: "POST", body: text });
}
