/** How long a server gets to shut down after SIGTERM before it is SIGKILLed. */
export const STOP_GRACE_MS = 4000
/**
 * How long after SIGKILL to keep waiting for the tree to disappear (a process
 * stuck in the kernel can't die at once) before giving up on it.
 */
export const KILL_GRACE_MS = 2000

/** Milliseconds on a clock that only goes forward. */
export const now = () => performance.now()
