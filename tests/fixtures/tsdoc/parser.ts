/**
 * Reads configuration files.
 *
 * @packageDocumentation
 */

import { EventEmitter } from 'node:events';

/**
 * How a parser reads a file.
 *
 * @public
 */
export interface Options {
  /**
   * The most entries to read.
   *
   * @defaultValue `100`
   */
  limit: number;
}

/**
 * Reads entries out of a file's text.
 *
 * @remarks
 * Call {@link Parser.parse | parse} once for each file, with {@link Options}. It follows
 * {@link pkg#Reader.read}, {@link @scope/pkg#Reader} and {@link (Parser:constructor)}, and
 * {@link Parser.(parse:instance)} is the one to call.
 *
 * @typeParam T - The shape of an entry.
 * @public
 */
export class Parser<T> extends EventEmitter {
  /**
   * Parses one file's text.
   *
   * @param text - The file's contents.
   * @param options - How to read it.
   * @returns The entries it found.
   * @throws {@link SyntaxError} When the text is empty.
   *
   * @example
   * ```ts
   * new Parser().parse('a = 1', { limit: 1 });
   * ```
   */
  parse(text: string, options: Options): T[] {
    if (!text) {
      throw new SyntaxError('empty');
    }
    return [];
  }
}

/**
 * A parser for plain text entries.
 *
 * @beta
 */
export class TextParser extends Parser<string> {
  /** {@inheritDoc Parser.parse} */
  override parse(text: string, options: Options): string[] {
    return super.parse(text, options);
  }

  /**
   * {@inheritDoc}
   */
  override emit(event: string): boolean {
    return super.emit(event);
  }
}

/**
 * Maps each entry to something else.
 *
 * @typeParam T - The shape of an entry.
 * @typeParam U - What each entry becomes.
 * @param entries - The entries to map.
 * @param to - What to map each one to.
 * @returns The mapped entries, in order.
 * @public
 */
export function map<T, U>(entries: T[], to: (entry: T) => U): U[] {
  return entries.map(to);
}
