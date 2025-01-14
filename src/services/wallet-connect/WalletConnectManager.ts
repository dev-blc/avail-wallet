
import {emit, once, emitTo} from '@tauri-apps/api/event';
import {WebviewWindow} from '@tauri-apps/api/webviewWindow';
import { getAll } from '@tauri-apps/api/window';
import {Core} from '@walletconnect/core';
import {formatJsonRpcError, type JsonRpcError, type JsonRpcResult} from '@walletconnect/jsonrpc-utils';
import {type SignClientTypes, type Verify} from '@walletconnect/types';
import {buildApprovedNamespaces, getSdkError} from '@walletconnect/utils';
import {Web3Wallet, type IWeb3Wallet, type Web3WalletTypes} from '@walletconnect/web3wallet';
import {get_address} from '../storage/persistent';
import {AleoWallet} from './AleoWallet';
import {SessionInfo} from './SessionInfo';
import {dappSession, type WalletConnectRequest} from './WCTypes';

type PingEventData = Omit<SignClientTypes.BaseEventArgs, 'params'>;

export class WalletConnectManager {
	theWallet?: IWeb3Wallet;
	projectId: string;
	relayerUrl: string;
	aleoWallet?: AleoWallet;
	clientId?: string;

	currentRequestVerifyContext?: Verify.Context;
	pairingTopic?: string;
	sessionTopic?: string;

	constructor() {
		this.projectId = '9d41eeacbfa8659cce91de12a8bf1806';
		this.relayerUrl = 'wss://relay.walletconnect.com';
		this.onSessionProposal = this.onSessionProposal.bind(this);
		this.onSessionDelete = this.onSessionDelete.bind(this);
		this.onSessionRequest = this.onSessionRequest.bind(this);
		// This.onAuthRequest = this.onAuthRequest.bind(this);
		this.onSignClientPing = this.onSignClientPing.bind(this);
	}

	async setup() {
		const address = await get_address();
		const wallet = new AleoWallet(address);
		this.aleoWallet = wallet;

		const core = new Core({
			projectId: this.projectId,
			relayUrl: this.relayerUrl,
		});

		this.theWallet = await Web3Wallet.init({
			core,
			metadata: {
				name: 'Avail',
				description: 'Frictionless control of your money and data privately on Aleo.',
				url: 'avail.global',
				icons: [],
			},
		});

		try {
			this.clientId = await this.theWallet.engine.signClient.core.crypto.getClientId();
			console.log('WalletConnect ClientID:', this.clientId);
		} catch (error) {
			console.error('Failed to set WalletConnect clientId', error);
		}

		this.theWallet.on('session_proposal', this.onSessionProposal);
		this.theWallet.on('session_request', this.onSessionRequest);
		// This.theWallet.on('auth_request', this.onAuthRequest);
		this.theWallet.engine.signClient.events.on('session_ping', this.onSignClientPing);
		this.theWallet.on('session_delete', this.onSessionDelete);
	}

	async pair(uri: string) {
		console.log('Pairing with...', uri);
		await this.setup();
		console.log('Setup with...', uri);
		if (!this.theWallet) {
			console.log('Wallet is null call setup()');
			return;
		}

		console.log('Pairing with...', uri);
		await this.theWallet?.pair({ uri });
	}

	async close() {
		/*
		If (this.pairingTopic) {
			console.log("Closing pairing...")
			await this.theWallet?.core.pairing.disconnect({topic : this.pairingTopic});
			await this.theWallet?.core.history.delete(this.pairingTopic);
		} */
		if (this.sessionTopic) {
			console.log('Closing pairing...');
			await this.theWallet?.disconnectSession({ topic: this.sessionTopic, reason: getSdkError('USER_DISCONNECTED') });
			// Await this.theWallet?.core.history.delete(this.sessionTopic);
		}

		await emitTo('main', 'disconnected', 'disconnected');

		console.log('Closing event handling...');
		this.theWallet?.off('session_proposal', this.onSessionProposal);
		this.theWallet?.off('session_request', this.onSessionRequest);
		// This.theWallet?.off('auth_request', this.onAuthRequest)
		this.theWallet?.engine.signClient.events.off('session_ping', this.onSignClientPing);
		this.theWallet?.off('session_delete', this.onSessionDelete);
	}

	// DApp sent us a session proposal
	//  id - is the dApp id submitting the proposal
	//  params - includes details of what the dApp is expecting.
	private async onSessionProposal(proposal: Web3WalletTypes.SessionProposal) {
		console.log();
		console.log();
		console.log('  ============================== ');
		console.log('  >>> session_proposal event >>>');
		console.log();

		try {
			if (!this.theWallet || !this.aleoWallet) {
				console.log('Wallet is null! Call setup()');
				return;
			}

			if (!proposal) {
				console.log('Missing proposal data.');
				return;
			}

			console.log('proposal', proposal);
			// TODO - Proposal.metadata is not being used it has data about the app we can display
			// metadata: { description: "example dapp", url: "",name:"",icons:["data:image/png;base64,...."] }

			const {metadata} = proposal.params.proposer;

			console.log('Windows -> ',getAll());

			/* Approve/Reject Connection window -- START */
			// Open the new window
			const webview = new WebviewWindow('wallet-connect', {
				url: 'wallet-connect-screens/wallet-connect.html',
				title: 'Avail Wallet Connect',
				width: 350,
				height: 600,
				resizable: false,
			});

			const wcRequest: WalletConnectRequest = {
				method: 'connect',
				question: 'Do you want to connect to ' + metadata.name + ' ?',
				imageRef: '../wc-images/connect.svg',
				approveResponse: 'User approved wallet connect',
				rejectResponse: 'User rejected wallet connect',
				description: metadata.description,
				dappUrl: metadata.url,
				dappImage: metadata.icons[0],
			};

			await webview.once('tauri://created', () => {
				console.log('Window created');

				console.log('Webview ', webview);

				setTimeout(async () => {
					await emit('wallet-connect-request', wcRequest);
					console.log('Emitting wallet-connect-request');
				}, 3000);
			});

			const {aleoWallet, theWallet} = this;

			await once('connect-approved', async response => {
				await webview.close();
				console.log('Wallet connect was approved', response);

				SessionInfo.show(proposal, [aleoWallet.chainName()]);

				const supportedNamespaces = {
					// What the dApp requested...
					proposal: proposal.params,

					// What we support...
					supportedNamespaces: {
						aleo: {
							chains: [aleoWallet.chainName()],
							methods: aleoWallet.chainMethods(),
							events: aleoWallet.chainEvents(),
							accounts: [`${aleoWallet.chainName()}:${aleoWallet.getAddress()}`],
						},
					},
				};
				console.log('supportedNamespaces', supportedNamespaces);
				const approvedNamespaces = buildApprovedNamespaces(supportedNamespaces);

				console.log('Approving session...');
				const session = await theWallet.approveSession({
					id: proposal.id,
					relayProtocol: proposal.params.relays[0].protocol,
					namespaces: approvedNamespaces,
				});
				console.log('Approved session', session);

				await emit('connected', session);

				this.currentRequestVerifyContext = proposal.verifyContext;

				// This value is present in the pairing URI
				// wc:<pairingTopic>@....
				this.pairingTopic = proposal.params.pairingTopic;

				// This value will stick throughout the session and will
				// be present in session_request, session_delete events
				this.sessionTopic = session.topic;
				console.log('Session topic', this.sessionTopic);

				const dappSess = dappSession(metadata.name, metadata.description, metadata.url, metadata.icons[0]);

				console.log('Storing dapp session', dappSess);
				sessionStorage.setItem(session.topic, JSON.stringify(dappSess));

			});

			// Listen for the rejection event from the secondary window
			await once('connect-rejected', async response => {
				// Handle the rejection logic here
				console.log('Wallet connect was rejected', response);
				await webview.close();
				throw new Error('User Rejected');
			});

			/* Approve/Reject Connection window -- END */
		} catch (error) {
			console.log('Rejecting session...');
			await this.theWallet?.rejectSession({
				id: proposal.id,
				reason: getSdkError('USER_REJECTED'),
			});

			console.log('Rejected. Error info...');
			console.log(error);
		} finally {
			console.log();
			console.log('  <<< session_proposal event <<<');
			console.log('  ============================== ');
			console.log();
		}
	}

	private async onSessionRequest(requestEvent: Web3WalletTypes.SessionRequest) {
		try {
			if (!this.theWallet || !this.aleoWallet) {
				console.log('Wallet is null! Call setup()');
				return;
			}

			if (!requestEvent) {
				console.log('Missing requestEvent data.');
				return;
			}

			console.log('request', requestEvent);

			const {topic} = requestEvent;
			const requestSession = this.theWallet.engine.signClient.session.get(requestEvent.topic);
			console.log('requestSession', requestSession);

			// Set the verify context so it can be displayed in the projectInfoCard
			this.currentRequestVerifyContext = requestEvent.verifyContext;

			// Call information chain | method
			const {chainId} = requestEvent.params;

			const requestMethod = requestEvent.params.request.method;
			console.log(`Handling request for ${chainId} | ${requestMethod}...`);

			let response: JsonRpcResult | JsonRpcError
                = formatJsonRpcError(requestEvent.id, `Chain unsupported ${chainId}`);

			if (chainId === this.aleoWallet.chainName()) {
				response = await this.aleoWallet.invokeMethod(requestEvent);
			} else {
				console.log(`Chain unsupported ${chainId}`);
			}

			console.log('Responding with...', response);
			await this.theWallet.respondSessionRequest({topic, response});
		} catch (error) {
			console.log('Failed', (error as Error).message);
			const {topic} = requestEvent;
			console.log('============>>>> Request event', requestEvent);
			await this.theWallet?.respondSessionRequest({topic, response: formatJsonRpcError(requestEvent.id, (error as Error).message)});
		} finally {
			console.log();
			console.log('  <<< session_request event <<<');
			console.log('  ============================= ');
			console.log();
		}
	}

	private async onSessionDelete(data: Web3WalletTypes.SessionDelete) {
		console.log('Event: session_delete received');
		console.log(data);
		//await this.close();
		await emit('disconnected', 'disconnected');
	}

	private onSignClientPing(data: PingEventData) {
		console.log('Event: session_ping received');
		console.log(data);
	}
}
