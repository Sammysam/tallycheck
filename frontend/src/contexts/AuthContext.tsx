import { createContext, useContext, useState, useEffect, type ReactNode } from 'react';
import axios from 'axios';
import api from '../api/client';

interface AuthUser {
  id: string;
  username: string;
  role: string;
  permissions: string[];
}

interface AuthContextType {
  user: AuthUser | null;
  tempToken: string | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  permissions: string[];
  hasPermission: (permission: string) => boolean;
  login: (username: string, password: string) => Promise<{ tempToken: string; requiresTwoFactor: boolean; isNewUser: boolean }>;
  completeAuth: (accessToken: string) => void;
  setTempToken: (token: string) => void;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType>(null!);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [tempToken, setTempToken] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const loadProfile = async () => {
    const { data } = await api.get('/auth/me');
    setUser({
      id: data.id,
      username: data.username,
      role: data.role?.name ?? '',
      permissions: data.role?.permissions ?? [],
    });
  };

  useEffect(() => {
    const token = localStorage.getItem('accessToken');
    if (token) {
      try {
        const payload = JSON.parse(atob(token.split('.')[1]));
        if (payload.exp * 1000 > Date.now() && payload.stage === 'authenticated') {
          setUser({ id: payload.sub, username: payload.username, role: '', permissions: [] });
          loadProfile()
            .catch(() => {
              localStorage.removeItem('accessToken');
              setUser(null);
            })
            .finally(() => setIsLoading(false));
          return;
        } else {
          localStorage.removeItem('accessToken');
        }
      } catch {
        localStorage.removeItem('accessToken');
      }
    }
    setIsLoading(false);
  }, []);

  const login = async (username: string, password: string) => {
    const { data } = await axios.post('/api/auth/login', { username, password });
    setTempToken(data.tempToken);
    return data;
  };

  const completeAuth = (accessToken: string) => {
    localStorage.setItem('accessToken', accessToken);
    try {
      const payload = JSON.parse(atob(accessToken.split('.')[1]));
      setUser({ id: payload.sub, username: payload.username, role: '', permissions: [] });
    } catch {}
    setTempToken(null);
    loadProfile().catch(() => {});
  };

  const logout = () => {
    localStorage.removeItem('accessToken');
    setUser(null);
    setTempToken(null);
  };

  const permissions = user?.permissions ?? [];
  const hasPermission = (permission: string) => user?.role === 'Admin' || permissions.includes(permission);

  return (
    <AuthContext.Provider value={{
      user,
      tempToken,
      isAuthenticated: !!user,
      isLoading,
      permissions,
      hasPermission,
      login,
      completeAuth,
      setTempToken,
      logout,
    }}>
      {children}
    </AuthContext.Provider>
  );
}

export const useAuth = () => useContext(AuthContext);
