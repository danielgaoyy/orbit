/**
 * Caches a `useSyncExternalStore` selector snapshot per immutable store state reference.
 *
 * Prevents React 19 infinite re-render loops (Minified React error #185) if a selector
 * inadvertently allocates a new object or array on repeated `getSnapshot()` calls for the same state.
 */
export function createCachedStateSnapshot<S, T>(
  getState: () => S,
  selector: (state: S) => T,
  onUnstableSelector?: () => void,
): () => T {
  let hasCache = false;
  let cachedState!: S;
  let cachedValue!: T;

  return () => {
    const currentState = getState();
    if (hasCache && Object.is(cachedState, currentState)) {
      return cachedValue;
    }
    const nextValue = selector(currentState);
    if (onUnstableSelector) {
      const checkValue = selector(currentState);
      if (!Object.is(nextValue, checkValue)) {
        onUnstableSelector();
      }
    }
    cachedState = currentState;
    cachedValue = nextValue;
    hasCache = true;
    return nextValue;
  };
}
