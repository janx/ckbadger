#!/usr/bin/env node
/**
 * Import known scripts from token-labels repo into ClickHouse.
 * Usage: node scripts/import-known-scripts.js [--clickhouse-url URL] [--dry-run]
 * Env: CLICKHOUSE_URL (default: http://localhost:8123)
 */

const fs = require('fs');
const path = require('path');
const https = require('https');
const http = require('http');

const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');
const clickhouseUrlArg = args.find((_, i) => args[i - 1] === '--clickhouse-url');
const CLICKHOUSE_URL = clickhouseUrlArg || process.env.CLICKHOUSE_URL || 'http://localhost:8123';
const DATABASE = 'ckbadger';

const SCRIPT_DIR = path.join(__dirname, '../docs/token-labels/information/script');
const OVERRIDES_FILE = path.join(__dirname, '../docs/script-name-overrides.json');

const SYSTEM_SCRIPT_NAMES = [
  'SECP256K1/blake160',
  'Default Lock',
  'Nervos DAO',
  'SECP256k1/Multisig',
  'Default Multisig',
];

const LOCK_SCRIPT_PATTERNS = [
  /lock/i,
  /secp256k1/i,
  /multisig/i,
  /pw.?lock/i,
  /omni.?lock/i,
  /anyone.?can.?pay/i,
  /joy.?id/i,
  /unipass/i,
  /das.?lock/i,
  /flash.?signer/i,
  /nostr/i,
  /time.?lock/i,
  /cheque/i,
  /^\.bit lock$/i,
];

const TYPE_SCRIPT_PATTERNS = [
  /udt/i,
  /spore/i,
  /nft/i,
  /dao/i,
  /cota/i,
  /ckbfs/i,
  /asset/i,
  /pool/i,
  /type.?id/i,
  /did/i,
  /account/i,
];

function detectScriptKind(name, decoderType) {
  if (['dao', 'udt', 'spore', 'spore-cluster'].includes(decoderType)) return 'type';
  for (const pattern of LOCK_SCRIPT_PATTERNS) if (pattern.test(name)) return 'lock';
  for (const pattern of TYPE_SCRIPT_PATTERNS) if (pattern.test(name)) return 'type';
  return null;
}

function loadOverrides() {
  try {
    return JSON.parse(fs.readFileSync(OVERRIDES_FILE, 'utf8'));
  } catch (e) {
    console.warn('Warning: Could not load overrides file:', e.message);
    return { overrides: {}, deprecated: [] };
  }
}

function readScriptFiles() {
  const scripts = [];
  for (const dir of fs.readdirSync(SCRIPT_DIR)) {
    const indexPath = path.join(SCRIPT_DIR, dir, 'index.json');
    if (fs.existsSync(indexPath)) {
      try {
        scripts.push({ dir, data: JSON.parse(fs.readFileSync(indexPath, 'utf8')) });
      } catch (e) {
        console.warn(`Warning: Could not parse ${indexPath}:`, e.message);
      }
    }
  }
  return scripts;
}

function escapeSql(str) {
  if (str === null || str === undefined) return '';
  return String(str).replace(/'/g, "''").replace(/\\/g, '\\\\');
}

function generateInserts(scripts, overrides) {
  const inserts = [];
  const deprecatedSet = new Set(overrides.deprecated.map((h) => h.toLowerCase()));

  for (const { dir, data } of scripts) {
    const baseName = data.name;
    const displayName = overrides.overrides[baseName] || baseName;
    const description = data.description || '';
    const rfc = data.rfc || '';
    const website = data.website || '';
    const sourceUrl = data.sourceUrl || '';
    const decoderType = data.decoderType || '';
    const isSystem =
      SYSTEM_SCRIPT_NAMES.includes(baseName) || SYSTEM_SCRIPT_NAMES.includes(displayName) ? 1 : 0;
    const scriptKind = detectScriptKind(displayName, decoderType) || '';

    for (const network of ['mainnet', 'testnet']) {
      const deployments = data.deployments?.[network] || [];

      for (const deployment of deployments) {
        const codeHash = deployment.codeHash || '';
        if (!codeHash) continue;

        const tag = deployment.tag || '';
        const hashType = deployment.hashType || '';
        const dataHash = deployment.dataHash || '';
        const typeHash = deployment.typeHash || '';
        const isDeprecated =
          deprecatedSet.has(codeHash.toLowerCase()) || deployment.deprecated ? 1 : 0;
        const codeCellTxHash = deployment.outPoint?.txHash || '';
        const codeCellOutputIndex = deployment.outPoint?.index ?? -1;

        const sql = `INSERT INTO ${DATABASE}.known_scripts (
          code_hash, network, tag, canon_version,
          name, description, rfc, website, source_url, decoder_type,
          hash_type, data_hash, type_hash, deprecated, is_system, script_kind,
          code_cell_tx_hash, code_cell_output_index,
          label_source, label_updated_at
        ) VALUES (
          unhex('${escapeSql(codeHash.replace(/^0x/, ''))}'),
          '${escapeSql(network)}',
          '${escapeSql(tag)}',
          1,
          '${escapeSql(displayName)}',
          '${escapeSql(description)}',
          '${escapeSql(rfc)}',
          '${escapeSql(website)}',
          '${escapeSql(sourceUrl)}',
          '${escapeSql(decoderType)}',
          '${escapeSql(hashType)}',
          unhex('${escapeSql(dataHash.replace(/^0x/, '') || '0'.repeat(64))}'),
          unhex('${escapeSql(typeHash.replace(/^0x/, '') || '0'.repeat(64))}'),
          ${isDeprecated},
          ${isSystem},
          '${escapeSql(scriptKind)}',
          ${codeCellTxHash ? `unhex('${escapeSql(codeCellTxHash.replace(/^0x/, ''))}')` : `unhex('${'0'.repeat(64)}')`},
          ${codeCellOutputIndex >= 0 ? codeCellOutputIndex : -1},
          'token-labels',
          now64(3)
        );`;

        inserts.push(sql);
      }
    }
  }

  return inserts;
}

async function executeSQL(sql) {
  return new Promise((resolve, reject) => {
    const url = new URL(CLICKHOUSE_URL);
    const options = {
      hostname: url.hostname,
      port: url.port || (url.protocol === 'https:' ? 443 : 8123),
      path: '/',
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
    };

    const client = url.protocol === 'https:' ? https : http;
    const req = client.request(options, (res) => {
      let data = '';
      res.on('data', (chunk) => (data += chunk));
      res.on('end', () => {
        if (res.statusCode >= 200 && res.statusCode < 300) resolve(data);
        else reject(new Error(`ClickHouse error (${res.statusCode}): ${data}`));
      });
    });

    req.on('error', reject);
    req.write(sql);
    req.end();
  });
}

async function main() {
  console.log('Known Scripts Import Tool');
  console.log('========================');
  console.log(`ClickHouse URL: ${CLICKHOUSE_URL}`);
  console.log(`Dry run: ${dryRun}\n`);

  console.log('Loading script data...');
  const overrides = loadOverrides();
  const scripts = readScriptFiles();
  console.log(`Found ${scripts.length} script definitions`);

  console.log('Generating INSERT statements...');
  const inserts = generateInserts(scripts, overrides);
  console.log(`Generated ${inserts.length} INSERT statements`);

  if (dryRun) {
    console.log('\n--- Dry run: SQL statements ---\n');
    for (const sql of inserts) console.log(sql + '\n');
    return;
  }

  console.log('\nTruncating existing known_scripts data...');
  try {
    await executeSQL(`TRUNCATE TABLE ${DATABASE}.known_scripts`);
    console.log('Truncated.');
  } catch (e) {
    console.warn('Warning: Could not truncate (table may not exist):', e.message);
  }

  console.log('\nInserting data...');
  let success = 0,
    failed = 0;

  for (const sql of inserts) {
    try {
      await executeSQL(sql);
      success++;
    } catch (e) {
      console.error('Failed:', e.message);
      console.error('SQL:', sql.substring(0, 200) + '...');
      failed++;
    }
  }

  console.log(`\nDone! Success: ${success}, Failed: ${failed}`);

  try {
    const count = await executeSQL(`SELECT count() FROM ${DATABASE}.known_scripts`);
    console.log(`Total rows in known_scripts: ${count.trim()}`);
  } catch (e) {
    console.warn('Could not verify:', e.message);
  }
}

main().catch((e) => {
  console.error('Fatal error:', e);
  process.exit(1);
});
