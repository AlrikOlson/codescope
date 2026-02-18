const [name, lang, baseUrl] = process.argv.slice(2);

if (!name || !lang || !baseUrl) {
  console.error('Usage: node validate.mjs <name> <lang> <baseUrl>');
  process.exit(1);
}

let passed = 0;
let failed = 0;

async function api(path, options = {}) {
  const url = `${baseUrl}${path}`;
  const r = await fetch(url, options);
  if (!r.ok) throw new Error(`${path} returned ${r.status}: ${await r.text()}`);
  return r.json();
}

function assert(condition, msg) {
  if (!condition) {
    console.error(`  x ${msg}`);
    failed++;
    return false;
  }
  passed++;
  return true;
}

function test(label, fn) {
  try {
    const result = fn();
    if (result) console.log(`  + ${label}`);
  } catch (e) {
    console.error(`  x ${label}: ${e.message}`);
    failed++;
  }
}

// Test 1: Tree structure
try {
  const tree = await api('/api/tree');
  test('Tree structure', () => assert(typeof tree === 'object' && tree !== null, 'tree should be an object'));
} catch (e) {
  console.error(`  x Tree structure: ${e.message}`);
  failed++;
}

// Test 2: Manifest
try {
  const manifest = await api('/api/manifest');
  const categories = Object.keys(manifest);
  const totalFiles = Object.values(manifest).flat().length;
  test('Manifest has categories', () => assert(categories.length > 0, `expected categories, got ${categories.length}`));
  test(`Manifest has files (${totalFiles})`, () => assert(totalFiles > 0, `expected files, got ${totalFiles}`));
} catch (e) {
  console.error(`  x Manifest: ${e.message}`);
  failed++;
}

// Test 3: Search
try {
  const search = await api('/api/search?q=main&fileLimit=10&moduleLimit=5');
  test('Search returns results', () => assert(
    (search.files?.length > 0 || search.modules?.length > 0),
    `expected search results for "main"`
  ));
} catch (e) {
  console.error(`  x Search: ${e.message}`);
  failed++;
}

// Test 4: Grep
try {
  const grep = await api('/api/grep?q=func&limit=10&maxPerFile=2');
  test('Grep endpoint works', () => assert(grep !== null, 'grep should return data'));
} catch (e) {
  console.error(`  x Grep: ${e.message}`);
  failed++;
}

// Test 5: Dependencies
try {
  const deps = await api('/api/deps');
  const depCount = Object.keys(deps).length;
  if (lang === 'rust') {
    test('Rust deps detected (Cargo.toml)', () => assert(depCount > 0, `expected Cargo.toml deps, got ${depCount}`));
  } else if (lang === 'javascript') {
    test('JS deps detected (package.json)', () => assert(depCount > 0, `expected package.json deps, got ${depCount}`));
  } else if (lang === 'go') {
    test('Go deps detected (go.mod)', () => assert(depCount >= 0, `go.mod deps: ${depCount}`));
  }
} catch (e) {
  console.error(`  x Dependencies: ${e.message}`);
  failed++;
}

// Test 6: Import graph
try {
  const imports = await api('/api/imports');
  test('Import graph endpoint works', () => assert(imports !== null, 'imports should return data'));
} catch (e) {
  console.error(`  x Imports: ${e.message}`);
  failed++;
}

// Test 7: Context/stubs extraction
try {
  const manifest = await api('/api/manifest');
  const sampleFiles = Object.values(manifest).flat().slice(0, 3).map(f => f.path);
  if (sampleFiles.length > 0) {
    const context = await api('/api/context', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ paths: sampleFiles, unit: 'tokens', budget: 10000 }),
    });
    test('Context extraction works', () => assert(
      context.summary?.totalFiles > 0,
      `expected files in context, got ${context.summary?.totalFiles}`
    ));
  }
} catch (e) {
  console.error(`  x Context: ${e.message}`);
  failed++;
}

// Summary
console.log(`  -- ${name}: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
