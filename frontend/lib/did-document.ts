/**
 * A did:ckb document as the API decoded it (CBOR → lossless JSON), read for people (ID-D1 /
 * ID-D2): its handles (`alsoKnownAs`, AT-protocol `at://` URIs), its services and its signing
 * keys. The document is owner-written; a field present in a shape this reader does not know is
 * listed in `unreadable` (the page then points at the stored document) rather than guessed at.
 */
export interface DidDocumentView {
  /** Handles without their `at://` scheme, in document order. */
  handles: string[];
  services: { name: string; endpoint: string; type: string }[];
  signingKeys: { name: string; key: string }[];
  unreadable: string[];
}

const isMap = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === 'object' && !Array.isArray(v);

/** The API's lossless tags for CBOR the JSON model cannot carry plainly (`$map`, `$tag`, …). */
const TAGGED = ['$map', '$tag', '$bytes', '$integer', '$float'];

export function readDidDocument(document: unknown): DidDocumentView {
  const view: DidDocumentView = { handles: [], services: [], signingKeys: [], unreadable: [] };
  // A document that is not a plain text-keyed map is reported whole, never read as empty.
  if (!isMap(document) || Object.keys(document).some((k) => TAGGED.includes(k)))
    return { ...view, unreadable: ['document'] };
  const aka = document.alsoKnownAs;
  if (aka !== undefined) {
    if (Array.isArray(aka) && aka.every((h) => typeof h === 'string'))
      view.handles = (aka as string[]).map((h) => h.replace(/^at:\/\//, ''));
    else view.unreadable.push('alsoKnownAs');
  }
  const services = document.services;
  if (services !== undefined) {
    if (!isMap(services)) view.unreadable.push('services');
    else
      for (const [name, service] of Object.entries(services)) {
        if (
          isMap(service) &&
          typeof service.endpoint === 'string' &&
          typeof service.type === 'string'
        )
          view.services.push({ name, endpoint: service.endpoint, type: service.type });
        else view.unreadable.push(`services.${name}`);
      }
  }
  const methods = document.verificationMethods;
  if (methods !== undefined) {
    if (!isMap(methods)) view.unreadable.push('verificationMethods');
    else
      for (const [name, key] of Object.entries(methods)) {
        if (typeof key === 'string') view.signingKeys.push({ name, key });
        else view.unreadable.push(`verificationMethods.${name}`);
      }
  }
  return view;
}

/** A handle's first label and the rest, for the title's rose domain suffix. */
export function splitHandle(handle: string): { name: string; suffix: string } {
  const dot = handle.indexOf('.');
  return dot <= 0
    ? { name: handle, suffix: '' }
    : { name: handle.slice(0, dot), suffix: handle.slice(dot) };
}
