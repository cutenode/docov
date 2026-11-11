/** Fetches `url`, trying again up to maxRetries times when it fails. */
export async function fetchWithRetry(url: string, maxRetries = 3): Promise<Response> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await fetch(url);
    } catch (error) {
      if (attempt >= maxRetries) throw error;
    }
  }
}
