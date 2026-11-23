import { render as renderHome } from './views/home.js';
import { render as renderPc } from './views/pc.js';
import { render as renderMedia } from './views/media.js';
import { render as renderShortcuts } from './views/shortcuts.js';
import { render as renderAi } from './views/ai.js';
import { render as renderAgenda } from './views/agenda.js';
import { render as renderWeather } from './views/weather.js';
import { render as renderGithub } from './views/github.js';
import { render as renderTimer } from './views/timer.js';

const views = {
  home: renderHome, pc: renderPc, media: renderMedia, shortcuts: renderShortcuts,
  ai: renderAi, agenda: renderAgenda, weather: renderWeather, github: renderGithub, timer: renderTimer
};

function init() {
  console.log('App initialized');
}
init();
