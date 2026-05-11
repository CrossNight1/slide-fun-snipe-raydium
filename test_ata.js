const { PublicKey } = require('@solana/web3.js');
const { getAssociatedTokenAddress } = require('@solana/spl-token');

(async () => {
  const wsol = new PublicKey('So11111111111111111111111111111111111111112');
  const feeTo = new PublicKey('11MAn3qpNfq24q2iEA46oj6QbG2XP71kr7sh1zsxyfp');
  const ata = await getAssociatedTokenAddress(wsol, feeTo);
  console.log("ATA for 11MAn3qpNfq24q2iEA46oj6QbG2XP71kr7sh1zsxyfp is:", ata.toBase58());
})();
