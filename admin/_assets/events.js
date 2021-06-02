export default class EventEmitter {
	constructor() {
		this.listeners = {};
	}
	
	addEventListener(name, listener) {
		if (!this.listeners.hasOwnProperty(name))
			this.listeners[name] = [];
		
		this.listeners[name].push(listener);
	}
	
	removeEventListener(name, listener) {
		if (this.listeners.hasOwnProperty(name))
			this.listeners[name] = this.listeners[name].filter(l => l != listener);
	}
	
	dispatchEvent(name, ...args) {
		if (this.listeners.hasOwnProperty(name)) {
			for (let listener of this.listeners[name]) {
				listener(...args);
			}
		}
	}
}
