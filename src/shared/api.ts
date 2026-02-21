import { createContext, useContext } from 'react';

/** Whether we're running inside Tauri (search window) or the web UI. */
const ApiContext = createContext<{ tauri: boolean }>({ tauri: false });

export const ApiProvider = ApiContext.Provider;
export const useIsTauri = () => useContext(ApiContext).tauri;

export default ApiContext;
