import { createContext, useContext } from 'react';

const ApiContext = createContext<{ baseUrl: string }>({ baseUrl: '' });

export const ApiProvider = ApiContext.Provider;
export const useApiBase = () => useContext(ApiContext).baseUrl;
export const apiUrl = (base: string, path: string) => base ? `${base}${path}` : path;

export default ApiContext;
