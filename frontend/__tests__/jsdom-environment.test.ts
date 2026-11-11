import { delay, http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';

// The test realm (__tests__/jsdom-environment.ts) has one set of fetch and abort primitives, as a
// browser's single realm does: Node's fetch, Request, Response, Headers, AbortController,
// AbortSignal, URL and URLSearchParams, the DOM being jsdom's. Before it, the jsdom environment
// gave the realm jsdom's AbortController next to Node's Request, and Node 24's fetch refuses a
// foreign signal: every react-router navigation (it builds `new Request(url, { signal })`) threw.

describe('the test realm: one set of fetch and abort primitives', () => {
  it("builds a Request with the realm's AbortController signal, and aborting it aborts the request", () => {
    const controller = new AbortController();
    const request = new Request('http://localhost:3000/blocks', { signal: controller.signal });
    expect(request.signal.aborted).toBe(false);

    const reason = new Error('navigated away');
    controller.abort(reason);
    expect(request.signal.aborted).toBe(true);
    expect(request.signal.reason).toBe(reason);
  });

  it("gives what fetch makes the realm's own classes", () => {
    expect(new Request('http://localhost:3000/').signal).toBeInstanceOf(AbortSignal);
    expect(new Response('').headers).toBeInstanceOf(Headers);
    expect(new URL('http://localhost:3000/?a=1').searchParams).toBeInstanceOf(URLSearchParams);
    // fetch recognises the realm's URLSearchParams as a form body (another implementation's would
    // be sent as its string, as text/plain)
    const form = new Request('http://localhost:3000/', {
      method: 'POST',
      body: new URLSearchParams({ a: '1' }),
    });
    expect(form.headers.get('content-type')).toBe(
      'application/x-www-form-urlencoded;charset=UTF-8'
    );
  });

  it("aborts a fetch in flight through MSW with the realm's AbortController", async () => {
    let started!: () => void;
    const handlerStarted = new Promise<void>((resolve) => {
      started = resolve;
    });
    server.use(
      http.get('/api/:network/v1/realm-abort', async () => {
        started();
        await delay('infinite');
        return HttpResponse.json({});
      })
    );

    const controller = new AbortController();
    const response = fetch('/api/mainnet/v1/realm-abort', { signal: controller.signal });
    // a fetch that cannot be built rejects here instead of reaching the handler
    await Promise.race([handlerStarted, response]);
    const reason = new Error('left the page');
    controller.abort(reason);
    await expect(response).rejects.toBe(reason);
  });

  it("lets jsdom's addEventListener take the realm's signal, on an element and on the window", () => {
    const controller = new AbortController();
    const onElement = vi.fn();
    const onWindow = vi.fn();
    const onceOnly = vi.fn();
    const button = document.createElement('button');
    button.addEventListener('click', onElement, { signal: controller.signal });
    button.addEventListener('click', onceOnly, { once: true, signal: controller.signal });
    window.addEventListener('realm-test', onWindow, { signal: controller.signal });

    button.click();
    button.click();
    window.dispatchEvent(new Event('realm-test'));
    controller.abort();
    button.click();
    window.dispatchEvent(new Event('realm-test'));

    expect(onElement).toHaveBeenCalledTimes(2);
    expect(onceOnly).toHaveBeenCalledTimes(1);
    expect(onWindow).toHaveBeenCalledTimes(1);
  });

  it('adds no listener with a signal that is already aborted', () => {
    const controller = new AbortController();
    controller.abort();
    const listener = vi.fn();
    document.addEventListener('realm-test', listener, { signal: controller.signal });
    document.dispatchEvent(new Event('realm-test'));
    expect(listener).not.toHaveBeenCalled();
  });

  it('still refuses a signal that is no AbortSignal', () => {
    // jsdom's own TypeError (its window's realm, so matched by message, not by class)
    expect(() =>
      document.addEventListener('realm-test', () => {}, { signal: {} as AbortSignal })
    ).toThrow("parameter 3 dictionary has member 'signal' that is not of type 'AbortSignal'");
  });
});
