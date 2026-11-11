import { builtinEnvironments, type Environment } from 'vitest/environments';

// The test realm: Vitest's jsdom environment with one set of fetch and abort primitives, as a
// browser's single realm has (vitest.config.mts names this file as the environment).
//
// Vitest 2's jsdom environment copies jsdom's window onto the global. Its key list puts jsdom's
// AbortController, AbortSignal, Headers, URL and URLSearchParams over Node's, while fetch, Request
// and Response stay Node's (undici), as jsdom has none. Node's Request then makes its own signal
// from the global (jsdom's) AbortController, yet checks a signal it is given against Node's own
// AbortSignal: Node 22's undici only duck-typed RequestInit.signal, Node 24's brand-checks it and
// throws "RequestInit: Expected signal to be an instance of AbortSignal" for every
// `new Request(url, { signal: new AbortController().signal })`, which react-router builds on each
// navigation.
//
// So Node's own five stay in place over jsdom's, and jsdom's addEventListener, whose `signal`
// option takes only jsdom's AbortSignal, follows a Node one with a jsdom controller. Vitest 4.0's
// jsdom environment does both (jsdom-keys.ts "Conflict with Node.js values", and
// patchAddEventListener); this file goes when Vitest is upgraded to it.

/** Node's fetch takes or makes these; jsdom defines its own. */
const NODE_FETCH_PRIMITIVES = [
  'AbortController',
  'AbortSignal',
  'Headers',
  'URL',
  'URLSearchParams',
] as const;

/** The members of jsdom's window this file uses (jsdom ships no types). */
interface JsdomWindow {
  AbortController: typeof AbortController;
  AbortSignal: typeof AbortSignal;
  EventTarget: typeof EventTarget;
}

/**
 * Let jsdom's addEventListener take the realm's (Node's) AbortSignal: each one is followed by a
 * jsdom AbortSignal, which jsdom is given instead. Anything else goes to jsdom as it is, so a
 * signal that is no AbortSignal is still refused by jsdom.
 */
function followNodeSignals(window: JsdomWindow, NodeAbortSignal: typeof AbortSignal): void {
  const JsdomAbortController = window.AbortController;
  const addEventListener = window.EventTarget.prototype.addEventListener;
  const followers = new WeakMap<AbortSignal, AbortSignal>();

  function follower(signal: AbortSignal): AbortSignal {
    let followed = followers.get(signal);
    if (followed === undefined) {
      const controller = new JsdomAbortController();
      if (signal.aborted) controller.abort(signal.reason);
      else signal.addEventListener('abort', () => controller.abort(signal.reason), { once: true });
      followed = controller.signal;
      followers.set(signal, followed);
    }
    return followed;
  }

  window.EventTarget.prototype.addEventListener = function (
    this: EventTarget,
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: boolean | AddEventListenerOptions
  ): void {
    if (typeof options === 'object' && options.signal instanceof NodeAbortSignal) {
      const { capture, once, passive } = options;
      return addEventListener.call(this, type, listener, {
        capture,
        once,
        passive,
        signal: follower(options.signal),
      });
    }
    return addEventListener.call(this, type, listener, options);
  };
}

const jsdom = builtinEnvironments.jsdom;

export default {
  name: 'jsdom',
  transformMode: 'web',
  async setup(global, options) {
    const node = NODE_FETCH_PRIMITIVES.map((key) => {
      const descriptor = Object.getOwnPropertyDescriptor(global, key);
      if (descriptor === undefined) {
        throw new Error(`jsdom-environment: Node defines no global ${key} to keep over jsdom's`);
      }
      return [key, descriptor] as const;
    });
    const NodeAbortSignal = global.AbortSignal as typeof AbortSignal;
    const jsdomOptions: Record<string, unknown> = options.jsdom ?? {};
    const beforeParse = jsdomOptions.beforeParse as ((window: JsdomWindow) => void) | undefined;

    const environment = await jsdom.setup(global, {
      ...options,
      jsdom: {
        ...jsdomOptions,
        // before the environment binds the window's functions onto the global
        beforeParse(window: JsdomWindow) {
          followNodeSignals(window, NodeAbortSignal);
          beforeParse?.(window);
        },
      },
    });
    // The builtin teardown deletes these keys and puts its saved originals (Node's) back.
    for (const [key, descriptor] of node) Object.defineProperty(global, key, descriptor);
    return environment;
  },
} satisfies Environment;
