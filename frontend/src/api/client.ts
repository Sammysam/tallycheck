import axios from 'axios';

const api = axios.create({ baseURL: '/api' });

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('accessToken');
  if (token) config.headers.Authorization = `Bearer ${token}`;
  return config;
});

api.interceptors.response.use(
  (r) => r,
  (err) => {
    if (err.response?.status === 401 && !err.config?.url?.includes('/auth/')) {
      localStorage.removeItem('accessToken');
      window.location.href = '/login';
    }
    return Promise.reject(err);
  },
);

/**
 * Readable text for an API error. Validation failures come back as an array
 * of per-field messages, so join them rather than rendering "[object Object]".
 */
export function errMsg(err: any, fallback = 'Something went wrong'): string {
  const message = err?.response?.data?.message;
  if (Array.isArray(message)) return message.join(' · ');
  if (typeof message === 'string' && message) return message;
  return err?.message || fallback;
}

export default api;
