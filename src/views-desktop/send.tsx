import * as React from 'react';
import * as mui from '@mui/material';
import {useNavigate} from 'react-router-dom';

// Services
import {useTranslation} from 'react-i18next';
import {emit} from '@tauri-apps/api/event';
import {transfer} from '../services/transfer/transfers';
import {getTokenBalance} from '../services/states/utils';
import {os} from '../services/util/open';
import {listen} from '@tauri-apps/api/event';
import {getNetwork, get_address, getUsername} from '../services/storage/persistent';

// Components
import TransferBox from '../components/transfer/transfer_box';
import MiniDrawer from '../components/sidebar';
import CTAButton from '../components/buttons/cta';
import SettingsComponent from '../components/switch/privacy_toggle';
import TransferDialog from '../components/dialogs/transfer';
import TransferInProgressDialog from '../components/dialogs/transfer_in_progress';
import ProfileBar from '../components/account/profile-header';

// Images
import aleo from '../assets/icons/tokens/aleo.svg';
import usdt from '../assets/icons/tokens/usdt.svg';
import {SmallText400} from '../components/typography/typography';

// Alerts
import {
	ErrorAlert, SuccessAlert, WarningAlert, InfoAlert,
} from '../components/snackbars/alerts';

// Types
import {type TransferRequest, TransferType} from '../types/transfer_props/tokens';
import {type AvailError, AvailErrorType} from '../types/errors';
import {getAuthType} from '../services/storage/persistent';

// Context
import {useScan} from '../context/ScanContext';
import Layout from './reusable/layout';

// TODO - Get tokens
const tokens = [
	{
		symbol: 'ALEO',
		image_url: aleo,
	}
];

const mockTransferRequest: TransferRequest = {
	asset_id: 'ALEO',
	amount: 10,
	recipient: 'test',
	transfer_type: TransferType.TransferPublic,
	message: 'test',
	fee_private: false,
	password: undefined,
	fee: 290_000,
};

function Send() {
	const [openDialog, setOpenDialog] = React.useState(false);
	const [response, setResponse] = React.useState<string>();
	const [biometric, setBiometric] = React.useState<boolean>(false);

	// Balance states
	const [privateBalance, setPrivateBalance] = React.useState<number>(0);
	const [publicBalance, setPublicBalance] = React.useState<number>(0);

	// Transfer states
	const [token, setToken] = React.useState<string>('ALEO');
	const [recipient, setRecipient] = React.useState<string>('');
	const [amount, setAmount] = React.useState<number>(0);
	const [transferMessage, setTransferMessage] = React.useState<string>('');
	const [request, setRequest] = React.useState<TransferRequest>(mockTransferRequest);
	const [TransferDialogOpen, setTransferDialogOpen] = React.useState(false);
	const [TransferInProgressDialogOpen, setTransferInProgressDialogOpen] = React.useState(false);

	// Privacy flags
	const [isPrivateTransferFrom, setIsPrivateTransferFrom] = React.useState(false);
	const [isPrivateTransferTo, setIsPrivateTransferTo] = React.useState(false);
	const [isPrivateFee, setIsPrivateFee] = React.useState(false);

	// Alert states
	const [errorAlert, setErrorAlert] = React.useState(false);
	const [successAlert, setSuccessAlert] = React.useState(false);
	const [warningAlert, setWarningAlert] = React.useState(false);
	const [infoAlert, setInfoAlert] = React.useState(false);
	const [message, setMessage] = React.useState('');

	// Profile  bar states
	const [address, setAddress] = React.useState('');
	const [username, setUsername] = React.useState('');
	const [network, setNetwork] = React.useState('');

	// Scan states
	const {scanInProgress, startScan, endScan} = useScan();
	const navigate = useNavigate();

	const {t} = useTranslation();

	const getAuth = async () => {
		const auth = await getAuthType();
		if (auth === 'true') {
			setBiometric(true);
		} else {
			setBiometric(false);
		}
	};

	React.useEffect(() => {
		// Set network

		getNetwork().then(res => {
			setNetwork(res);
		}).catch(error => {
			console.log(error);
			setMessage('Failed to get network.');
			setErrorAlert(true);
		});

		// Set address
		get_address().then(res => {
			setAddress(res);
		}).catch(error => {
			console.log(error);
			setMessage('Failed to get address.');
			setErrorAlert(true);
		});

		// Set username
		getUsername().then(res => {
			setUsername(res);
		}).catch(error => {
			console.log(error);
			setMessage('Failed to get username.');
			setErrorAlert(true);
		});
	}, []);

	/* --Event Listners */
	React.useEffect(() => {
		const unlistenTx = listen('tx_in_progress_notification', event => {
			setTransferInProgressDialogOpen(true);
		});

		return () => {
			unlistenTx.then(remove => {
				remove();
			}).catch(error => {
				console.log(error);
				setMessage('Error listening to tx_in_progress_notification event.');
				setErrorAlert(true);
			});
		};
	}, []);

	const handleTransfer = async () => {
		let transferType: TransferType;

		if (isPrivateTransferFrom && isPrivateTransferTo) {
			transferType = TransferType.TransferPrivate;
		} else if (isPrivateTransferFrom && !isPrivateTransferTo) {
			transferType = TransferType.TransferPrivateToPublic;
		} else if (!isPrivateTransferFrom && isPrivateTransferTo) {
			transferType = TransferType.TransferPublicToPrivate;
		} else {
			transferType = TransferType.TransferPublic;
		}

		if (amount === undefined || recipient === '' || token === '') {
			setMessage(t('send.messages.error.fields'));
			setErrorAlert(true);
			return;
		}

		if (amount === 0) {
			setMessage(t('send.messages.error.zero-amount'));
			setErrorAlert(true);
			return;
		}

		if (amount < 0) {
			setMessage(t('send.messages.error.positive-amount'));
			setErrorAlert(true);
			return;
		}

		if (amount > privateBalance && isPrivateTransferFrom) {
			setMessage(t('send.messages.error.insufficient-private-amount'));
			setErrorAlert(true);
			return;
		}

		if (amount > publicBalance && !isPrivateTransferFrom) {
			setMessage(t('send.messages.error.insufficient-public-amount'));
			setErrorAlert(true);
			return;
		}

		let asset_id = token;

		if (token === 'ALEO') {
			asset_id = 'credits';
		}

		const request: TransferRequest = {
			asset_id,
			amount: amount * 1_000_000,
			recipient,
			transfer_type: transferType,
			message: transferMessage,
			fee_private: isPrivateFee,
			password: undefined,
			fee: 297_000,
		};

		sessionStorage.setItem('transferState', 'true');
		transfer(request, setErrorAlert, setMessage).then(res => {
			sessionStorage.setItem('transferState', 'false');
		}).catch(async err => {
			console.log(err);
			const error = err as AvailError;

			sessionStorage.setItem('transferState', 'false');
			if (error.error_type.toString() === 'Unauthorized') {
				sessionStorage.setItem('transferState', 'false');
				await emit('transfer_off');

				setRequest(request);
				setTransferDialogOpen(true);
			}
			// TODO - Handle insufficient balance error
		});
	};

	const shouldRunEffect = React.useRef(true);
	React.useEffect(() => {
		let assetId = token;

		if (token === 'ALEO') {
			assetId = 'credits';
		}

		getTokenBalance(assetId).then(res => {
			if (res.balances !== undefined) {
				const balances = res.balances[0];
				setPrivateBalance(balances.private);
				setPublicBalance(balances.public);
			}
		}).catch(error => {
			console.log(error);
			setMessage('Failed to get token balances.');
			setErrorAlert(true);
		});
	}, [token]);

	// TODO : Get list of tokens owned by user and display them in a dropdown + amounts available of each
	return (
		<Layout>
			<ErrorAlert errorAlert={errorAlert} setErrorAlert={setErrorAlert} message={message} />
			<SuccessAlert successAlert={successAlert} setSuccessAlert={setSuccessAlert} message={message} />
			<WarningAlert warningAlert={warningAlert} setWarningAlert={setWarningAlert} message={message} />
			<InfoAlert infoAlert={infoAlert} setInfoAlert={setInfoAlert} message={message} />

			{/* TransferInProgress Dialog */}
			<TransferInProgressDialog isOpen={TransferInProgressDialogOpen} onRequestClose={() => {
				setTransferInProgressDialogOpen(false);
			}} />

			{/* Transfer Dialog */}
			<TransferDialog isOpen={TransferDialogOpen} onRequestClose={() => {
				setTransferDialogOpen(false);
			}} request={request} />
			<MiniDrawer />
			<mui.Box sx={{
				width: '100%', height: '100%', display: 'flex', justifyContent: 'center', alignContent: 'center', flexDirection: 'column',
			}}>
				<mui.Box sx={{
					display: 'flex', flexDirection: 'row', justifyContent: 'flex-end', mt: '2%', mr: '5%', alignItems: 'center',
				}}>
					<mui.Chip label={network} variant='outlined' sx={{mr: '2%', color: '#a3a3a3'}} />
					<ProfileBar address={address} name={username}></ProfileBar>
				</mui.Box>
				<mui.Box sx={{
					display: 'flex', width: '45%', bgcolor: '#00A07D', borderRadius: 9, mt: '8%', alignSelf: 'center'
				}}>
					<mui.Box sx={{
						display: 'flex', flexDirection: 'column', alignSelf: 'center', width: '100%', borderRadius: 9, backdropFilter: 'blur(10px)',
						background: 'radial-gradient(ellipse at center, #00A07D -20%, #2A3331 110%)',
						boxShadow: `
            0 0 60px 0 rgba(0, 255, 190, 0.6),  // Soft green glow
            0 0 100px 0 rgba(0, 255, 190, 0.4),  // Medium green glow
            0 0 150px 0 rgba(0, 255, 190, 0.2)   // Wide green glow
          `,
						p: 3,
					}}>

						<mui.Box sx={{width: '85%', alignSelf: 'center'}}>
							<TransferBox tokens={tokens} token={token} amount={amount} setToken={setToken} setAmount={setAmount} />

						</mui.Box>
						{/* private balance */}
						<mui.Box sx={{display: 'flex', flexDirection: 'column', mt: '2%'}}>
							<mui.Box sx={{
								display: 'flex', flexDirection: 'row', justifyContent: 'space-between', alignSelf: 'center', width: '80%',
							}}>
								{/* private balance* and fee */}
								<mui.Box sx={{display: 'flex', flexDirection: 'row', width: '60%'}}>
									<SmallText400 sx={{color: '#fff', mr: '2%'}}>{t('send.private-balance')}</SmallText400>
									<SmallText400 sx={{color: '#fff'}}>{privateBalance}</SmallText400>
								</mui.Box>
								{/* TODO - Fetch fee from microservice. */}
								<mui.Box sx={{display: 'flex', flexDirection: 'row'}}>
									<SmallText400 sx={{color: '#fff'}}>{t('send.fee')}: 0.29</SmallText400>
								</mui.Box>
							</mui.Box>

							{/* public balance */}
							<mui.Box sx={{
								display: 'flex', flexDirection: 'row', alignSelf: 'center', width: '80%',
							}}>
								<SmallText400 sx={{color: '#fff', mr: '2%'}}>{t('send.public-balance')}</SmallText400>
								<SmallText400 sx={{color: '#fff'}}>{publicBalance}</SmallText400>
							</mui.Box>

						</mui.Box>
						<mui.TextField
							id='outlined-basic'
							variant='outlined'
							onChange={e => {
								setRecipient(e.target.value);
							}}
							value={recipient}
							placeholder='@Username or Aleo address...'
							sx={{
								width: '85%',
								height: '40px',
								alignSelf: 'center',
								backgroundColor: '#3E3E3E',
								borderRadius: '15px',
								mt: '8%',
								'& .MuiOutlinedInput-root': {
									'& fieldset': {
										border: 'none',
									},
								},
							}}
							inputProps={{style: {color: '#fff', height: '10px'}}}
							InputLabelProps={{style: {color: '#fff'}}}
						/>

						<mui.TextField
							id='outlined-basic'
							label=''
							variant='outlined'
							onChange={e => {
								setTransferMessage(e.target.value);
							}}
							value={transferMessage}
							placeholder='Add a message...'
							sx={{
								width: '85%',
								height: '40px',
								alignSelf: 'center',
								backgroundColor: '#3E3E3E',
								borderRadius: '15px',
								mt: '2%',
								'& .MuiOutlinedInput-root': {
									'& fieldset': {
										border: 'none',
									},
								},
								boxShadow: 'none',
							}}
							inputProps={{style: {color: '#fff', height: '10px'}}}
							InputLabelProps={{style: {color: '#fff'}}}
						/>
						<SettingsComponent onTransferFromToggle={value => {
							setIsPrivateTransferFrom(value);
						}} onTransferToToggle={value => {
							setIsPrivateTransferTo(value);
						}} onFeeToggle={value => {
							setIsPrivateFee(value);
						}} />
						<mui.Button
							onClick={async () => {
								await handleTransfer();
							}}
							variant='contained'
							autoCapitalize='false'
							sx={{
								backgroundColor: '#00FFAA', width: '50%', borderRadius: '30px',
								display: 'flex', justifyContent: 'center', alignContent: 'center', alignItems: 'center', textTransform: 'none', alignSelf: 'center', marginTop: '5%',
								transition: 'transform 0.1s ease-in-out, box-shadow 0.1s ease-in-out',
								'&:hover': {
									backgroundColor: '#00FFAA',
									boxShadow: '0 0 8px 2px rgba(0, 255, 170, 0.6)',
									transform: 'scale(1.03)',
								},
								'&:focus': {
									backgroundColor: '#00FFAA',
									boxShadow: '0 0 8px 2px rgba(0, 255, 170, 0.8)',
								},
							}}>
							<mui.Typography sx={{fontSize: '1.2rem', color: '#000', fontWeight: 450}}>
								{t('send.send')}
							</mui.Typography>
						</mui.Button>
					</mui.Box>

				</mui.Box>
			</mui.Box>

		</Layout>
	);
}

export default Send;
