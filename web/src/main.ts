import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { loadWasm } from './lib/wasm';

loadWasm().then(() => {
  mount(App, { target: document.getElementById('app')! });
});

if ('serviceWorker' in navigator && import.meta.env.PROD) {
  navigator.serviceWorker.register('./sw.js').catch(() => {});
}
