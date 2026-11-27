/**
 * This browser's guest session with the match server. The server keeps it in an `HttpOnly` cookie that lasts the
 * session's lifetime, so every tab, a reopened browser, the HTTP calls, and the match WebSocket share one guest.
 * The page holds only the CSRF token and expiry it recovers from the server; it stores no credential itself.
 */
export interface BrowserSession {
  account_id: string
  display_name: string
  csrf_token: string
  expires_at_ms: number
}

/** The match server could not say whether this browser has a session; nothing was created. */
export class SessionUnavailableError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'SessionUnavailableError'
  }
}

const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]'])

function isSession(value: unknown): value is BrowserSession {
  if (typeof value !== 'object' || value === null) return false
  const v = value as Record<string, unknown>
  return (
    typeof v.account_id === 'string' &&
    v.account_id !== '' &&
    typeof v.display_name === 'string' &&
    typeof v.csrf_token === 'string' &&
    v.csrf_token !== '' &&
    Number.isSafeInteger(v.expires_at_ms)
  )
}

/**
 * The match server's base URL as this page should reach it. A cookie travels only within one site, and `localhost`
 * and `127.0.0.1` are different sites, so on a loopback setup the server is addressed by the page's own loopback name.
 */
export function matchServerBase(): string {
  const configured = useRuntimeConfig().public.matchServerUrl
  if (typeof window === 'undefined') return configured
  try {
    const url = new URL(configured)
    const page = window.location.hostname
    if (LOOPBACK.has(url.hostname) && LOOPBACK.has(page) && url.hostname !== page) {
      // Only the host changes; a configured base path stays.
      url.hostname = page
      return url.href.replace(/\/$/, '')
    }
  } catch {
    // A configured value that is not a URL is used as given.
  }
  return configured
}

/** The session this tab last confirmed, reused by actions until the server refuses it. */
let known: BrowserSession | null = null

export function useBrowserSession() {
  const base = matchServerBase()

  /**
   * The browser's existing session, or null only when the server answers that it has none (expired, revoked, or
   * never created). Any other failure throws, so it is never mistaken for an absent session. With `fresh`, the server
   * is asked even when this tab confirmed a session before, as every page does when it opens.
   */
  async function recover(options: { fresh?: boolean } = {}): Promise<BrowserSession | null> {
    if (!options.fresh && known && known.expires_at_ms > Date.now()) return known
    let resp: Response
    try {
      resp = await fetch(`${base}/api/v1/auth/session`, { credentials: 'include', cache: 'no-store' })
    } catch (error) {
      throw new SessionUnavailableError(`The match server did not answer: ${String(error)}`)
    }
    if (resp.status === 401) {
      known = null
      return null
    }
    if (!resp.ok) throw new SessionUnavailableError(`The match server could not check the session (${resp.status})`)
    const body: unknown = await resp.json().catch(() => null)
    // Only a well-formed session is one; anything else from a successful answer is unavailable, never "no session".
    if (!isSession(body)) throw new SessionUnavailableError('The match server answered without a session')
    known = body
    return known
  }

  /**
   * The existing session, or a new guest only when the server confirms there is none. Creation runs under a lock
   * shared by this browser's tabs, which checks again inside it, so two tabs opened together share one guest.
   */
  async function ensure(displayName = 'Jade Guest'): Promise<BrowserSession> {
    const existing = await recover({ fresh: true })
    if (existing) return existing
    const create = async () => {
      const again = await recover({ fresh: true })
      if (again) return again
      const created = await fetch(`${base}/api/v1/auth/guest`, {
        method: 'POST',
        credentials: 'include',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ display_name: displayName, transport: 'web_cookie' }),
      })
      if (!created.ok) throw new SessionUnavailableError(`Guest session failed (${created.status})`)
      // Read back through the cookie, so the session used is the one every other tab will also recover.
      const session = await recover({ fresh: true })
      if (!session) throw new SessionUnavailableError('This browser did not keep the guest session cookie')
      return session
    }
    const locks = typeof navigator !== 'undefined' ? navigator.locks : undefined
    return locks ? await locks.request('jade-guest-session', create) : await create()
  }

  /**
   * The session for an action: the one this tab confirmed, or a fresh check when it has none. An action never creates
   * a guest; with no session it stops, and only opening a room or creating an invitation makes one.
   */
  async function current(): Promise<BrowserSession> {
    if (known && known.expires_at_ms > Date.now()) return known
    const session = await recover({ fresh: true })
    if (!session) throw new SessionUnavailableError('Your guest session has ended; reload to continue as a new guest')
    return session
  }

  /**
   * Forgets the confirmed session when the server refused it (no session, an expired one, or a CSRF mismatch), so
   * the next action asks the server again. Returns whether the response was such a refusal.
   */
  async function refusedBy(resp: Response): Promise<boolean> {
    if (resp.status === 401) {
      known = null
      return true
    }
    if (resp.status !== 403) return false
    const code = await resp
      .clone()
      .json()
      .then((body: { error?: { code?: string } }) => body.error?.code)
      .catch(() => undefined)
    if (code === 'csrf_missing' || code === 'csrf_mismatch') {
      known = null
      return true
    }
    return false
  }

  /** Headers for a cookie-authenticated mutation: the cookie travels with `credentials: 'include'`. */
  function mutationHeaders(session: BrowserSession): Record<string, string> {
    return { 'content-type': 'application/json', 'x-csrf-token': session.csrf_token }
  }

  return { recover, ensure, current, refusedBy, mutationHeaders, base }
}
