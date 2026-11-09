/** Gets the display name. */
export function getDisplayName(user: { name?: string; email: string }): string {
  return user.name ?? user.email;
}
