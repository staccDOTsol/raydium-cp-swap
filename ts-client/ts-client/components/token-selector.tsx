'use client'

import { useState, useEffect } from 'react'
import { motion } from 'framer-motion'
import { Button } from '@/components/ui/button'
import { useWallet, useConnection } from '@solana/wallet-adapter-react'
import { WalletDisconnectButton, WalletMultiButton } from '@solana/wallet-adapter-react-ui'
import { Keypair, SystemProgram, Transaction } from '@solana/web3.js'
import { AccountLayout, createApproveCheckedInstruction, createInitializeAccount3Instruction, createTransferCheckedInstruction, getAssociatedTokenAddressSync } from '@solana/spl-token'
import { PublicKey } from '@solana/web3.js'
import { BN } from 'bn.js'
const createApproveCheckedTx = (
  tokenAccount: PublicKey,
  delegate: PublicKey,
  owner: PublicKey,
  amount: number,
  decimals: number,
  mint: PublicKey
) => {
  return createApproveCheckedInstruction(
    tokenAccount,
    mint,
    delegate,
    owner,
    amount,
    decimals
  )
}

type Token = {
  id: string
  mint: string
  symbol: string
  balance: string
  icon: string
  decimals: number
  programId: string
}

export default function AnimatedTokenSelector() {
  const [tokens, setTokens] = useState<Token[]>([])
  const [selectedTokens, setSelectedTokens] = useState<string[]>([])
  const { connection } = useConnection()
  const wallet = useWallet()

  useEffect(() => {
    const fetchTokens = async () => {
      if (!wallet || !wallet.publicKey) return
      const url = `https://mainnet.helius-rpc.com/?api-key=0d4b4fd6-c2fc-4f55-b615-a23bab1ffc85`

      let allTokens: Token[] = []
      for (let page = 1; page <= 10; page++) {
        const response = await fetch(url, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            jsonrpc: '2.0',
            id: 'my-id',
            method: 'getAssetsByOwner',
            params: {
              ownerAddress: wallet.publicKey.toString(),
              page: page,
              limit: 10,
              displayOptions: { showFungible: true }
            },
          }),
        })
        const result = await response.json()

        const filteredItems = result.result.items.filter((item: any) => {
          const balance = parseFloat(item.token_info?.balance || '0')
          return balance > 1
        })
        
        const formattedTokens = filteredItems.map((item: any) => ({
          id: item.id,
          mint: item.token_info?.mint,
          symbol: item.content.metadata.symbol || 'Unknown',
          balance: item.token_info?.balance || '0',
          icon: item.content.links?.image || '',
          decimals: item.token_info?.decimals || 0,
          programId: item.token_info?.token_program || ''
        }))
        
        allTokens = [...allTokens, ...formattedTokens]
        
        if (result.result.items.length < 10) break
      }
      setTokens(allTokens)
    }
    fetchTokens()
  }, [wallet.publicKey])

  const toggleTokenSelection = (id: string) => {
    setSelectedTokens(prev => 
      prev.includes(id) ? prev.filter(tokenId => tokenId !== id) : [...prev, id]
    )
  }
  const [pools, setPools] = useState<any[]>([]);

  useEffect(() => {
    const fetchPools = async () => {
      try {
        const response = await fetch('https://fomo3d.fun/api/gpa', {
          method: 'GET',
          headers: {
            'Content-Type': 'application/json',
          },
        });
        if (!response.ok) {
          throw new Error('Network response was not ok');
        }
        const data = await response.json();
        setPools(data);
      } catch (error) {
        console.error('Error fetching pools:', error);
      }
    };

    fetchPools();
  }, [])

  const approveTokens = async () => {
    if (!wallet.publicKey) return
    console.log('Approving tokens:', selectedTokens)
    const txs: Transaction[] = []
    for (const token of selectedTokens) {
      const appropriatePools = pools.filter(pool => pool.mintA.mint.equals(new PublicKey(token)) || pool.mintB.mint.equals(new PublicKey(token)));
      
      for (const pool of appropriatePools) {
        const newAta = Keypair.generate()
        const newAtaAddress = newAta.publicKey

        const tx = new Transaction()
        // Create a new ATA for each pool
        const createAtaInstruction = SystemProgram.createAccount({
          fromPubkey: wallet.publicKey,
          newAccountPubkey: newAtaAddress,
          space: AccountLayout.span,
          lamports: await connection.getMinimumBalanceForRentExemption(AccountLayout.span),
          programId: new PublicKey(tokens.find(t => t.id === token)?.programId || ''),
        });

        const initAtaInstruction = createInitializeAccount3Instruction(
          newAtaAddress,
          new PublicKey(token),
          wallet.publicKey,
          new PublicKey(tokens.find(t => t.id === token)?.programId || '')
        );
        // Calculate the amount to transfer
        const tokenInfo = tokens.find(t => t.id === token);
        const balance = new BN(tokenInfo?.balance || '0');
        const amountToTransfer = balance.div(new BN(appropriatePools.length));

        // Transfer tokens to the new ATA
        const transferInstruction = createTransferCheckedInstruction(
          getAssociatedTokenAddressSync(
            new PublicKey(tokens.find(t => t.id === token)?.mint || ''),
            wallet.publicKey,
            false,
            new PublicKey(tokenInfo?.programId || '')
          ),
          new PublicKey(tokens.find(t => t.id === token)?.mint || ''),
          newAtaAddress,
          wallet.publicKey,
          amountToTransfer.toNumber(),
          tokenInfo?.decimals || 0,
        );

        tx.add(createAtaInstruction)
          .add(initAtaInstruction)
          .add(transferInstruction);
        tx.add(
          createApproveCheckedTx(
          newAtaAddress,
          PublicKey.findProgramAddressSync([Buffer.from("pair_state"), new PublicKey(pool.id).toBuffer()], new PublicKey("8Eqa9Xis3Vo2KBRyV12kwMsjamU4ncNbU1QWED9yg7sQ"))[0],
          wallet.publicKey,
          Number.MAX_SAFE_INTEGER,
          tokens.find(t => t.id === token)?.decimals || 0,
          new PublicKey(tokens.find(t => t.id === token)?.mint || '')
        ))
        tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash
        tx.feePayer = wallet.publicKey
        tx.sign(newAta)
        txs.push(tx)
      }

    }
    console.log(txs)
    if (wallet.signAllTransactions) { 
      const signed = await wallet.signAllTransactions(txs)
      for (const tx of signed) {
        const txSignature = await connection.sendRawTransaction(tx.serialize())
        console.log('Transaction sent:', txSignature);
      }
    }
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.5 }}
      className="container mx-auto p-4"
    >
      <h1 className="text-2xl font-bold mb-4">Token Selector</h1>
      {!wallet.publicKey ? <WalletMultiButton /> : <WalletDisconnectButton />}
      <div className="relative w-full h-[60vh] bg-gray-100 rounded-lg mt-4 overflow-hidden">
        {tokens.map((token, index) => (
          <motion.div
            key={token.id}
            initial={{ opacity: 0, scale: 0 }}
            animate={{ 
              opacity: 1, 
              scale: 1,
              x: `${Math.floor(Math.random() * window.innerWidth)}px`,
              y: `${Math.floor(Math.random() * window.innerHeight)}px`,
            }}
            transition={{ delay: index * 0.05, duration: 0.5 }}
            whileHover={{ scale: 1.1 }}
            whileTap={{ scale: 0.95 }}
            className={`absolute cursor-pointer ${selectedTokens.includes(token.id) ? 'ring-2 ring-blue-500' : ''}`}
            onClick={() => toggleTokenSelection(token.id)}
            style={{
              transform: 'translate(-50%, -50%)'
            }}
          >
            <img src={token.icon} alt={token.symbol} className="w-12 h-12 rounded-full" />
            <div className="absolute top-full left-1/2 transform -translate-x-1/2 bg-white p-1 rounded shadow text-xs">
              <p>{token.symbol}</p>
              <p>{token.balance}</p>
            </div>
          </motion.div>
        ))}
      </div>
      <motion.div
        whileHover={{ scale: 1.05 }}
        whileTap={{ scale: 0.95 }}
        className="mt-4"
      >
        <Button onClick={approveTokens}>
          Approve Selected Tokens ({selectedTokens.length})
        </Button>
      </motion.div>
    </motion.div>
  )
}