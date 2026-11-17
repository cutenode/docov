/**
 * Reads a text one line at a time.
 *
 * @param text - The text to read.
 * @yields Each line, without its line break.
 */
export function* lines(text: string): Generator<string> {
  for (const line of text.split("\n")) {
    yield line;
  }
}
