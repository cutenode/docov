/** The name to show for a user: theirs, or their email when they haven't set one. */
export function getDisplayName(user: { name?: string; email: string }): string {
  return user.name ?? user.email;
}
