/**
 * Parses a port number.
 *
 * @param value - The port, as text.
 * @returns The port.
 * @throws {RangeError} When it isn't a whole number from 1 to 65535.
 */
export function parsePort(value: string): number {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new RangeError(`not a port: ${value}`);
  }
  return port;
}
