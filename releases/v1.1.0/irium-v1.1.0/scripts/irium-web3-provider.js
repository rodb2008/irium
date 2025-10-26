/**
 * Irium Web3 Provider for External Wallet Integration
 * Compatible with MetaMask, Trust Wallet, and other Web3 wallets
 */

class IriumWeb3Provider {
    constructor(apiUrl = 'https://207.244.247.86/api') {
        this.apiUrl = apiUrl;
        this.chainId = '0x1'; // Irium mainnet chain ID
        this.chainName = 'Irium Mainnet';
        this.nativeCurrency = {
            name: 'Irium',
            symbol: 'IRM',
            decimals: 8
        };
        this.rpcUrls = [apiUrl];
        this.blockExplorerUrls = ['https://207.244.247.86'];
        // Use official logo from GitHub
        this.iconUrls = ['http://207.244.247.86:8080/irium-logo-wallet.svg'];
    }

    // Web3 Provider Methods
    async request(method, params = []) {
        switch (method) {
            case 'eth_accounts':
                return await this.getAccounts();
            case 'eth_requestAccounts':
                return await this.requestAccounts();
            case 'eth_getBalance':
                return await this.getBalance(params[0]);
            case 'eth_sendTransaction':
                return await this.sendTransaction(params[0]);
            case 'eth_getTransactionCount':
                return '0x0'; // Nonce for new transactions
            case 'eth_chainId':
                return this.chainId;
            case 'net_version':
                return '1';
            case 'eth_blockNumber':
                return '0x0'; // Genesis block
            default:
                throw new Error(`Method ${method} not supported`);
        }
    }

    async getAccounts() {
        try {
            const response = await fetch(`${this.apiUrl}/wallet/addresses`, {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json',
                },
            });
            const data = await response.json();
            return data.data.addresses.map(addr => this.toEthereumAddress(addr));
        } catch (error) {
            console.error('Error getting accounts:', error);
            return [];
        }
    }

    async requestAccounts() {
        const accounts = await this.getAccounts();
        if (accounts.length === 0) {
            throw new Error('No accounts found. Please create a wallet first.');
        }
        return accounts;
    }

    async getBalance(address) {
        try {
            const response = await fetch(`${this.apiUrl}/wallet/balance`, {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json',
                },
            });
            const data = await response.json();
            // Convert IRM to wei (8 decimals)
            const balanceWei = (data.data.balance * 100000000).toString(16);
            return '0x' + balanceWei;
        } catch (error) {
            console.error('Error getting balance:', error);
            return '0x0';
        }
    }

    async sendTransaction(txParams) {
        // This is a placeholder - in a real implementation, you'd sign and broadcast
        console.log('Transaction request:', txParams);
        return '0x' + Math.random().toString(16).substr(2, 64); // Mock tx hash
    }

    // Utility method to convert Irium addresses to Ethereum format
    toEthereumAddress(iriumAddress) {
        // This is a simplified conversion - in reality, you'd need proper address conversion
        const hash = this.simpleHash(iriumAddress);
        return '0x' + hash.substring(0, 40);
    }

    simpleHash(str) {
        let hash = 0;
        for (let i = 0; i < str.length; i++) {
            const char = str.charCodeAt(i);
            hash = ((hash << 5) - hash) + char;
            hash = hash & hash; // Convert to 32-bit integer
        }
        return Math.abs(hash).toString(16).padStart(40, '0');
    }

    // Add Irium network to MetaMask
    async addToMetaMask() {
        if (typeof window.ethereum !== 'undefined') {
            try {
                await window.ethereum.request({
                    method: 'wallet_addEthereumChain',
                    params: [{
                        chainId: this.chainId,
                        chainName: this.chainName,
                        nativeCurrency: this.nativeCurrency,
                        rpcUrls: this.rpcUrls,
                        blockExplorerUrls: this.blockExplorerUrls,
                        iconUrls: this.iconUrls
                    }]
                });
                return true;
            } catch (error) {
                console.error('Error adding Irium to MetaMask:', error);
                return false;
            }
        }
        return false;
    }
}

// Usage example
if (typeof window !== 'undefined') {
    window.IriumWeb3Provider = IriumWeb3Provider;
    
    // Auto-inject if MetaMask is available
    if (typeof window.ethereum !== 'undefined') {
        const iriumProvider = new IriumWeb3Provider();
        
        // Add Irium network to MetaMask
        window.addIriumToMetaMask = () => iriumProvider.addToMetaMask();
        
        console.log('Irium Web3 Provider loaded. Use addIriumToMetaMask() to add the network.');
    }
}

// Node.js export
if (typeof module !== 'undefined' && module.exports) {
    module.exports = IriumWeb3Provider;
}
