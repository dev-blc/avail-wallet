import React from "react";
import ReactDOM from "react-dom/client";
import {
  createBrowserRouter,
  RouterProvider,
  Route,
  Link
} from "react-router-dom";

import App from "./App";

// screens
import Register from "./views-desktop/register";
import HomeDesktop from "./views-desktop/home-desktop";
import Login from "./views-desktop/login";
import Verify from "./views-desktop/verify";
import Settings from "./views-desktop/settings";
import Activity from "./views-desktop/activity";
import BrowserView from "./views-desktop/browser";
import Send from "./views-desktop/send";
import Recovery from "./views-desktop/recovery";
import SeedPhrase from "./views-desktop/seedphrase";

// global font styles
import "./index.css";

//global states
import { ScanProvider } from "./context/ScanContext";
import { WalletConnectProvider } from "./context/WalletConnect";
import { RecentEventsProvider } from "./context/EventsContext";

//languages
import i18n from './i18next-config'; 

// see if language is set in local storage
const storedLanguage = localStorage.getItem('language');

if (storedLanguage) {
  i18n.changeLanguage(storedLanguage);
} else {
  i18n.changeLanguage('en');
}


const router = createBrowserRouter([
  { path: "/", element: <App /> }, //MVP 
  { path: "/register", element: <Register /> },//MVP
  { path: "/home", element: <HomeDesktop /> },//MVP
  { path: "/login", element: <Login /> },//MVP
  { path: "/send", element: <Send /> }, // MVP ? TBD
  { path: "/recovery", element: <Recovery /> }, // MVP
  { path: "/seed", element: <SeedPhrase /> }, // MVP 
  { path: "/verify", element: <Verify /> }, // MVP
  { path: "/settings", element: <Settings /> }, // MVP
  { path: "*", element: <HomeDesktop /> },
  { path: "/activity", element: <Activity /> },
  { path: "/browser", element: <BrowserView /> },
  {path: "/support", element: <a href="discord://EeuhRNwx"/>}
]);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(

  <React.StrictMode >
    <WalletConnectProvider>
      <ScanProvider>
        <RecentEventsProvider>
        <RouterProvider router={router} />
        </RecentEventsProvider>
      </ScanProvider>
    </WalletConnectProvider>
  </React.StrictMode>

);