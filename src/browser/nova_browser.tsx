import React, {type ReactEventHandler, useState} from 'react';

// Tauri tools
import {listen} from '@tauri-apps/api/event';

// Styles
import {
	AppBar, Toolbar, IconButton, InputBase, Paper, Box, Button, Grid, Typography,
} from '@mui/material';
import ArrowBackIosNewIcon from '@mui/icons-material/ArrowBackIosNew';
import RefreshIcon from '@mui/icons-material/Refresh';
import {styled} from '@mui/material/styles';

// global state
import {useTranslation} from 'react-i18next';
import {useWalletConnectManager} from '../context/WalletConnect';
import DappView from '../components/dApps/dapp';
import {Title2Text} from '../components/typography/typography';
import {dapps} from '../assets/dapps/dapps';
import {useScan} from '../context/ScanContext';

// Alerts
import {
	ErrorAlert, SuccessAlert, WarningAlert, InfoAlert,
} from '../components/snackbars/alerts';
import {open_url} from '../services/util/open';
import {useNavigate} from 'react-router-dom';

const Search = styled('div')(({theme}) => ({
	position: 'relative',
	borderRadius: theme.shape.borderRadius,
	backgroundColor: theme.palette.common.white,
	marginLeft: 0,
	width: '60%',
}));

type BrowserProperties = {
	initialUrl?: string;
	theme?: 'dark' | 'light';
	handleDappSelection: (url: string) => void;
};

const Browser: React.FC<BrowserProperties> = ({initialUrl, theme = 'light', handleDappSelection}) => {
	const [url, setUrl] = useState<string | undefined>(initialUrl ?? '');
	const [inputUrl, setInputUrl] = useState(url);
	const [previousUrls, setPreviousUrls] = useState<string[]>([]);
	const [wcUrl, setWcUrl] = useState<string>('');
	const [connected, setConnected] = useState<boolean>(false);
	const [showMenu, setShowMenu] = useState<boolean>(false);

	// Alert states
	const [errorAlert, setErrorAlert] = useState(false);
	const [successAlert, setSuccessAlert] = useState(false);
	const [warningAlert, setWarningAlert] = useState(false);
	const [infoAlert, setInfoAlert] = useState(false);
	const [alertMessage, setAlertMessage] = useState('');

	const navigate = useNavigate();
	const {walletConnectManager} = useWalletConnectManager();

	const {t} = useTranslation();

	const handleConnected = () => {
		walletConnectManager.pair(wcUrl).catch(() => {
			setAlertMessage('Error connecting');
			setErrorAlert(true);
		});
		sessionStorage.setItem('connected', 'true');
	};

	const getConnectState = () => {
		const connected = sessionStorage.getItem('connected');
		if (connected === 'true') {
			return true;
		}

		return false;
	};

	const handleInputChange = (event: React.ChangeEvent<HTMLInputElement>) => {
		setInputUrl(event.target.value);
	};

	const handleInputWcUrl = (event: React.ChangeEvent<HTMLInputElement>) => {
		setWcUrl(event.target.value);
	};

	const handleInputSubmit = (event: React.FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		if (inputUrl && inputUrl !== url && inputUrl !== '') {
			let urlModified = inputUrl;

			if (!inputUrl.startsWith('https://') && !inputUrl.startsWith('http://')) {
				urlModified = 'https://' + inputUrl;
			}

			setPreviousUrls([...previousUrls, url ?? '']);
			setUrl(urlModified);
			setShowMenu(false);

			if (urlModified !== 'https://faucet.puzzle.online') {
				sessionStorage.setItem('activeUrl', urlModified);
			}
		}
	};

	const handleDisconnect = () => {
		walletConnectManager.close().then(() => {
			sessionStorage.setItem('connected', 'false');
			setConnected(false);
			setAlertMessage(t('browser.message.success.disconnect'));
			setSuccessAlert(true);
		}).catch(error => {
			setConnected(false);
			sessionStorage.setItem('connected', 'false');
			setAlertMessage('Error disconnecting ');
			setErrorAlert(true);
		});
	};

	const handleBack = () => {
		setUrl(previousUrls.at(-1));
		setInputUrl(previousUrls.at(-1));
		setPreviousUrls(previousUrls.slice(0, -1));

		if (previousUrls.length === 0) {
			setInputUrl('');
			setUrl('');
			sessionStorage.removeItem('activeUrl');
			navigate('/dapps');
		}
	};

	const handleReload = () => {
		// Logic for reload action
		setUrl(inputUrl);
		const iframe = document.querySelector('iframe');
		if (iframe) {
			iframe.src = inputUrl || '';
		}
	};

	function handleUrlChangeInIframe(): ReactEventHandler<HTMLIFrameElement> {
		return (event) => {
			// if url starts with avail:// open in native app
			console.log('rip')
			const iframe = event.target as HTMLIFrameElement;
			const url = iframe.contentWindow?.location.href;
			if (url && url.startsWith('avail://')) {
				event.preventDefault();
				open_url(url);
			}
		};
	}

	const handleDappSelect = (url: string) => {
		setInputUrl(url);
		setUrl(url);
		setShowMenu(false);

		const iframe = document.querySelector('iframe');
		if (iframe) {
			iframe.src = url;
			if (url !== 'https://faucet.puzzle.online') {
				sessionStorage.setItem('activeUrl', url);
			}
		}
	};

	React.useEffect(() => {
		// Check for active url in session storage
		const activeUrl = sessionStorage.getItem('activeUrl');
		console.log('activeUrl', activeUrl);
		if (activeUrl && activeUrl !== 'https://faucet.puzzle.online') {
			setUrl(activeUrl);
			setInputUrl(activeUrl);
		}

		const connected = getConnectState();
		setConnected(connected);

		const unlistenConnected = listen('connected', event => {
			setConnected(true);
		});

		const unlistenDisconnected = listen('disconnected', event => {
			setConnected(false);
		});

		return () => {
			unlistenConnected.then(remove => {
				remove();
			}).catch(error => {
				console.log(error);
			});

			unlistenDisconnected.then(remove => {
				remove();
			}).catch(error => {
				console.log(error);
			});
		};
	}, []);

	return (
		<Box sx={{ml: '5%', height: url === '' ? '100%' : '94vh', width: '94%'}}>
			<ErrorAlert errorAlert={errorAlert} setErrorAlert={setErrorAlert} message={alertMessage} />
			<SuccessAlert successAlert={successAlert} setSuccessAlert={setSuccessAlert} message={alertMessage} />
			<AppBar position='static' sx={{bgcolor: '#111111'}} >
				<Toolbar variant='dense'>
					<IconButton edge='start' color='inherit' aria-label='back' onClick={handleBack}>
						<ArrowBackIosNewIcon />
					</IconButton>
					<IconButton color='inherit' aria-label='reload' onClick={handleReload}>
						<RefreshIcon />
					</IconButton>
					<Search>
						<Paper
							component='form'
							sx={{
								p: '2px 4px', display: 'flex', alignItems: 'center', width: '50%',
							}}
							onSubmit={handleInputSubmit}
						>
							<InputBase
								sx={{ ml: 1, flex: 1 }}
								placeholder={t('browser.enter') + ' URL'}
								inputProps={{ 'aria-label': 'enter url' }}
								value={inputUrl}
								onChange={handleInputChange}
							/>
						</Paper>
					</Search>
					<Box sx={{ width: '30%', ml: '2%' }}>
						<Paper
							component='form'
							sx={{
								p: '2px 4px', display: 'flex', alignItems: 'center', width: '80%',
							}}
							onSubmit={handleInputSubmit}
						>
							<InputBase
								sx={{ml: 1, flex: 1}}
								placeholder={t('browser.enter') + ' Wallet Connect Link'}
								inputProps={{'aria-label': 'enter url'}}
								value={wcUrl}
								onChange={handleInputWcUrl}
							/>
						</Paper>
					</Box>
					<Button sx={{
						borderRadius: '10px', width: '12%', bgcolor: '#00FFAA', color: '#111111',
						transition: 'transform 0.1s ease-in-out, box-shadow 0.1s ease-in-out',
						textTransform: 'none',
						'&:hover': {
							backgroundColor: '#00FFAA',
							boxShadow: '0 0 8px 2px rgba(0, 255, 170, 0.6)',
							transform: 'scale(1.03)',
						},
						'&:focus': {
							backgroundColor: '#00FFAA',
							boxShadow: '0 0 8px 2px rgba(0, 255, 170, 0.8)',
						},
					}}
					onClick={() => {
						connected ? handleDisconnect() : handleConnected();
					}}
					> {connected ? t('browser.message.success.disconnect') : t('browser.connect')}</Button>
				</Toolbar>
			</AppBar>
			{url !== ''
				&& <iframe
					src={url}
					title='Browser'
					width='100%'
					height='100%'
					loading='lazy'
					allowFullScreen
					allow='clipboard-read; clipboard-write'

				/>
			}
			{url === ''
				&& <Box sx={{
					display: 'flex', flexDirection: 'column', p: '20px', ml: '2%',
				}}>
					<Title2Text sx={{color: '#fff'}}> {t('browser.title')} </Title2Text>
					<Typography variant='body1' sx={{color: '#a3a3a3'}}>
						{t('browser.subtitle')}
					</Typography>
					<Grid container spacing={2} sx={{marginTop: '20px', alignItems: 'center'}}>
						{dapps.map((dapp, index) => (
							<Grid item xs={12} md={4} key={index}>
								<DappView dapp={dapp} onClick={() => {
									handleDappSelect(dapp.url);
									handleDappSelection(dapp.url);
								}} />
							</Grid>
						))}
					</Grid>

				</Box>
			}
		</Box>
	);
};

export default Browser;
