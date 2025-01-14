import React from 'react';
import * as mui from '@mui/material';
import {useNavigate} from 'react-router-dom';

// Types
import {type Task, type Quest} from '../../types/quests/quest_types';

// Typography
import {SubMainTitleText} from '../typography/typography';

// Components
import STButton from '../buttons/settings-button';

// Services
import {isTaskCompleted, verifyTask} from '../../services/quests/quests';

// Icons
import CheckCircleIcon from '@mui/icons-material/CheckCircle';

type TaskBoxProps = {
	task: Task;
	quest: Quest;
	questCompleted: boolean;
};

const TaskBox: React.FC<TaskBoxProps> = ({task, quest, questCompleted}) => {
	const [completed, setCompleted] = React.useState(false);
	const navigate = useNavigate();

	const shouldRunEffect = React.useRef(true);
	// Check if task has been complete

	React.useEffect(() => {
		if (shouldRunEffect.current) {
			if (questCompleted) {
				setCompleted(true);
			} else {
				isTaskCompleted(task.id).then(res => {
					console.log('Task complete ? : ', res);
					if (res) {
						setCompleted(true);
					} else if (task.program_id && task.function_id) {
						console.log('Verifying task');
						verifyTask(task.id, quest.created_on, quest.expires_on, task.program_id, task.function_id).then(res => {
							if (res) {
								setCompleted(true);
							}
						}).catch(err => {
							console.log(err);
						});
					}
				},
				).catch(err => {
					console.log(err);
				});
			}

			shouldRunEffect.current = false;
		}
	});

	return (
		<mui.Box sx={{
			bgcolor: '#111111',
			borderRadius: 5,
			padding: '20px',
			height: '100px',
			border: '1px solid #404040',
			mt: '2%',
			display: 'flex',
			flexDirection: 'row',
			alignItems: 'center',
			justifyContent: 'space-between',
			width: '80%',
			alignSelf: 'center',
		}}>
			<mui.Box sx={{display: 'flex', flexDirection: 'column'}}>
				<SubMainTitleText color='#FFF'>{task.title}</SubMainTitleText>
				<mui.Typography variant='body1' color='#fff' sx={{mb: '0%'}}>{task.description}</mui.Typography>
				<mui.Typography variant='h5' color='#fff' sx={{mt: '2%', color: '#00FFAA'}}>{task.points} Avail Points</mui.Typography>
			</mui.Box>
			{completed ? (
				<CheckCircleIcon sx={{color: '#00FFAA', width: '35px', height: '35px'}}/>
			) : (
				task.dapp_url === 'faucet' ? (
					<STButton text='Go to Faucet' onClick={() => {
						navigate('/faucet');
					}}/>
				) : task.dapp_url === 'transfer'
					? (
						<STButton text='Go to Transfer' onClick={() => {
							navigate('/send');
						}}/>
					)
					: (
						<STButton text='Go to DApp' onClick={() => {
							navigate('/browser', {state: task.dapp_url});
						}}
						/>
					)
			)}
		</mui.Box>
	);
};

export default TaskBox;
