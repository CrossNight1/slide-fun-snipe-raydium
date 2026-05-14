const { Connection } = require('@solana/web3.js');

(async () => {
  const conn = new Connection('https://api.devnet.solana.com');
  const tx = await conn.getTransaction('dicx3BFJAneMzgm48tFDtPZPCzZqUGYsGnyX7YCcYBQereac4N9xXyJkP7Khu87G2D8hwXtmwDd8BmUPaPVaNhT', { maxSupportedTransactionVersion: 0 });
  console.log(JSON.stringify(tx.meta.logMessages, null, 2));
})();
