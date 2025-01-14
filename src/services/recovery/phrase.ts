import {invoke} from '@tauri-apps/api/core';
import {type NavigateFunction} from 'react-router-dom';
import {type AvailError} from '../../types/errors';
import {type Languages} from '../../types/languages';

export async function recover(phrase: string, password: string, authType: boolean, language: Languages, navigate: NavigateFunction, setSuccessAlert: React.Dispatch<React.SetStateAction<boolean>>, setErrorAlert: React.Dispatch<React.SetStateAction<boolean>>, setMessage: React.Dispatch<React.SetStateAction<string>>) {
	return delete_local_for_recovery(password).then(() => {
		localStorage.clear();
		invoke<string>('recover_wallet_from_seed_phrase', {
			seed_phrase: phrase, password, access_type: authType, language,
		}).then(response => {
			setMessage('Wallet recovered successfully.');
			setSuccessAlert(true);

			setTimeout(() => {
				navigate('/home');
			}, 800);
		}).catch(async err => {
			const error = err as AvailError;

			console.log('Error: ' + error.internal_msg);

			setMessage('Recovery Failed: ' + error.external_msg);
			setErrorAlert(true);
		});
	}).catch(async err => {
		const error = err as AvailError;

		setMessage(error.external_msg);
		setErrorAlert(true);
	});
}

export async function delete_local_for_recovery(password: string) {
	return invoke('delete_local_for_recovery', {password});
}
