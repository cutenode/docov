/**
 * Formats a user as \{name\} \<email\>, or name\@host.
 */
export function formatUser(name: string, email: string): string {
  return `${name} <${email}>`;
}
