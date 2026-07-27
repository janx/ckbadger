import { MARKDOWN_ROUTE_PATTERNS } from '@/lib/ai/markdown-route';
import { RAW_ROUTE_PATTERNS } from '@/lib/ai/raw-route';
import {
  resolveApiBasePattern,
  resolveDefaultNetwork,
  resolveNetworks,
  resolveWsUrlPattern,
} from '@/lib/runtime-config';

const RAW_MEDIA_TYPE = 'application/vnd.ckbadger.raw+json';
const MARKDOWN_MEDIA_TYPE = 'text/markdown';

const RAW_DEFAULT_PROFILE = 'default';
const RAW_ROUTE_PROFILES: Record<string, readonly string[]> = {
  '/blocks/{id}': ['default'],
  '/cell/{outpoint}': ['default'],
  '/identities/dotbit/{identityId}': ['default'],
  '/identities/did/{identityId}': ['default'],
  '/objects/mnft/{objectId}': ['default'],
  '/tx/{hash}': ['default', 'debugger'],
};

export interface AiCapabilities {
  site: {
    name: 'ckbadger';
    pageBasePattern: '/{network}';
    /** Per-network API base, e.g. `/api/{network}/v1`. There is NO un-prefixed API path. */
    apiBasePattern: string;
    /** Per-network WebSocket URL, e.g. `/ws/{network}`. There is NO un-prefixed WS path. */
    wsUrlPattern: string;
    /** Networks this deployment serves — substitute one for `{network}` above. */
    networks: readonly string[];
    defaultNetwork: string;
  };
  formatNegotiation: {
    priority: ['query.format', 'path.suffix', 'accept.header'];
    supportedFormats: ['html', 'md', 'raw'];
    markdown: {
      suffix: '.md';
      query: 'format=md';
      accept: typeof MARKDOWN_MEDIA_TYPE;
    };
    raw: {
      suffix: '.raw';
      query: 'format=raw';
      accept: typeof RAW_MEDIA_TYPE;
      profileQuery: 'profile=<name>';
      defaultProfile: typeof RAW_DEFAULT_PROFILE;
    };
  };
  responseHeaders: {
    raw: {
      formatHeader: 'x-ckbadger-format';
      profileHeader: 'x-ckbadger-profile';
      schemaHeader: 'x-ckbadger-schema';
    };
  };
  responseMetadata: {
    markdown: {
      frontmatterFields: readonly [
        'title',
        'path',
        'canonical',
        'pageType',
        'generatedAt',
        'buildVersion',
        'formatVersion',
      ];
    };
    raw: {
      metaFields: readonly [
        'format',
        'profile',
        'schemaVersion',
        'buildVersion',
        'network',
        'path',
        'canonical',
        'pageType',
        'generatedAt',
      ];
    };
  };
  routes: {
    markdown: readonly string[];
    raw: readonly string[];
  };
  rawProfiles: {
    routes: Record<string, readonly string[]>;
    strictErrors: {
      invalidProfile: 'invalid_profile';
      profileNotSupported: 'profile_not_supported';
    };
    txDebuggerProfile: {
      route: '/tx/{hash}';
      profile: 'debugger';
      payloadPath: 'data.txDebugger.mockTransaction';
      debuggerCommandTemplate: string;
    };
    txWitnessPayload: {
      route: '/tx/{hash}';
      payloadPath: 'data.txWitness';
      fields: readonly ['available', 'witnessesCount', 'inputCount', 'analyses', 'inference'];
    };
  };
}

export function buildAiCapabilities(origin?: string): AiCapabilities & { origin?: string } {
  return {
    ...(origin ? { origin } : {}),
    site: {
      name: 'ckbadger',
      pageBasePattern: '/{network}',
      apiBasePattern: resolveApiBasePattern(),
      wsUrlPattern: resolveWsUrlPattern(),
      networks: resolveNetworks(),
      defaultNetwork: resolveDefaultNetwork(),
    },
    formatNegotiation: {
      priority: ['query.format', 'path.suffix', 'accept.header'],
      supportedFormats: ['html', 'md', 'raw'],
      markdown: {
        suffix: '.md',
        query: 'format=md',
        accept: MARKDOWN_MEDIA_TYPE,
      },
      raw: {
        suffix: '.raw',
        query: 'format=raw',
        accept: RAW_MEDIA_TYPE,
        profileQuery: 'profile=<name>',
        defaultProfile: RAW_DEFAULT_PROFILE,
      },
    },
    responseHeaders: {
      raw: {
        formatHeader: 'x-ckbadger-format',
        profileHeader: 'x-ckbadger-profile',
        schemaHeader: 'x-ckbadger-schema',
      },
    },
    responseMetadata: {
      markdown: {
        frontmatterFields: [
          'title',
          'path',
          'canonical',
          'pageType',
          'generatedAt',
          'buildVersion',
          'formatVersion',
        ],
      },
      raw: {
        metaFields: [
          'format',
          'profile',
          'schemaVersion',
          'buildVersion',
          'network',
          'path',
          'canonical',
          'pageType',
          'generatedAt',
        ],
      },
    },
    routes: {
      markdown: MARKDOWN_ROUTE_PATTERNS,
      raw: RAW_ROUTE_PATTERNS,
    },
    rawProfiles: {
      routes: RAW_ROUTE_PROFILES,
      strictErrors: {
        invalidProfile: 'invalid_profile',
        profileNotSupported: 'profile_not_supported',
      },
      txDebuggerProfile: {
        route: '/tx/{hash}',
        profile: 'debugger',
        payloadPath: 'data.txDebugger.mockTransaction',
        debuggerCommandTemplate:
          'curl "<url>.raw?profile=debugger" | jq \'.data.txDebugger.mockTransaction\' > mock_tx.json && ckb-debugger --tx-file mock_tx.json --cell-index 0 --cell-type input --script-group-type lock',
      },
      txWitnessPayload: {
        route: '/tx/{hash}',
        payloadPath: 'data.txWitness',
        fields: ['available', 'witnessesCount', 'inputCount', 'analyses', 'inference'],
      },
    },
  };
}
