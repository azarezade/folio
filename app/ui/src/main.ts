import './styles.css';
import { mount } from 'svelte';
import App from './App.svelte';

// `?gallery` shows the Folio component gallery instead of the app.
const gallery = new URLSearchParams(location.search).has('gallery');
const root = gallery ? (await import('./Gallery.svelte')).default : App;

export default mount(root, { target: document.getElementById('app')! });
