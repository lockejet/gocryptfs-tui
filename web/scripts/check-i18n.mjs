// Fails if the en/zh catalogs drift apart. Run via `npm run check-i18n`.
import { assertParity } from '../src/i18n.js';

try {
  const count = assertParity();
  console.log(`OK: ${count} keys`);
} catch (err) {
  console.error(`FAIL: ${err.message}`);
  process.exit(1);
}
