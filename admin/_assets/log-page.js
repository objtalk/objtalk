import { template } from "./utils.js";

const MAX_LOG_MESSAGES = 100;

class UuidColorer {
	constructor() {
		this.colors = ["#EB483F", "#90B136", "#E69B3D", "#5EA4D5", "#E1787B", "#6EBC9C", "#ED6F6B", "#C1E357", "#F7CB62", "#8DD5FB", "#F2A7AC", "#9CEECD"];
		this.assignments = [];
	}
	
	getColor(id) {
		let assignment = this.assignments.find(a => a.id == id);
		if (assignment) {
			return assignment.color;
		}
		
		if (this.colors.length > 0) {
			let color = this.colors.shift();
			this.assignments.push({ id, color })
			return color;
		}
		
		let { color } = this.assignments.shift();
		this.assignments.push({ id, color });
		return color;
	}
}

function shortId(id) {
	return id.slice(0, 7);
}

function objectUrl(name) {
	return "#objects/" + name;
}

function renderLogMessage(elem, message, colorer) {
	elem.querySelector(".log-message-client").innerText = shortId(message.client);
	elem.querySelector(".log-message-client").style.color = colorer.getColor(message.client);
	
	switch (message.type) {
		case "get":
			elem.querySelector(".log-message-pattern").innerText = message.pattern;
			break;
		case "query":
			elem.querySelector(".log-message-pattern").innerText = message.pattern;
			elem.querySelector(".log-message-query").innerText = shortId(message.query);
			elem.querySelector(".log-message-provide-rpc").innerText = "(provide rpc: "+!!message.provideRpc+")";
			break;
		case "unsubscribe":
			elem.querySelector(".log-message-query").innerText = shortId(message.query);
			break;
		case "set":
		case "patch":
			elem.querySelector(".log-message-object").innerText = message.object;
			elem.querySelector(".log-message-object").href = objectUrl(message.object);
			elem.querySelector(".log-message-value").innerText = JSON.stringify(message.value);
			break;
		case "remove":
			elem.querySelector(".log-message-object").innerText = message.object;
			elem.querySelector(".log-message-object").href = objectUrl(message.object);
			break;
		case "emit":
			elem.querySelector(".log-message-object").innerText = message.object;
			elem.querySelector(".log-message-object").href = objectUrl(message.object);
			elem.querySelector(".log-message-event").innerText = message.event;
			elem.querySelector(".log-message-data").innerText = JSON.stringify(message.data);
			break;
		case "invoke":
			elem.querySelector(".log-message-object").innerText = message.object;
			elem.querySelector(".log-message-object").href = objectUrl(message.object);
			elem.querySelector(".log-message-invocation").innerText = shortId(message.invocationId);
			elem.querySelector(".log-message-method").innerText = message.method;
			elem.querySelector(".log-message-args").innerText = JSON.stringify(message.args);
			break;
		case "invokeResult":
			elem.querySelector(".log-message-invocation").innerText = shortId(message.invocationId);
			elem.querySelector(".log-message-result").innerText = JSON.stringify(message.result);
			break;
	}
}

export default class LogPage {
	constructor(conn, system) {
		this.container = document.getElementById("log-messages");
		this.templates = {
			"clientConnect": template("template-log-message-client-connect"),
			"clientDisconnect": template("template-log-message-client-disconnect"),
			"get": template("template-log-message-get"),
			"query": template("template-log-message-query"),
			"unsubscribe": template("template-log-message-unsubscribe"),
			"set": template("template-log-message-set"),
			"patch": template("template-log-message-patch"),
			"remove": template("template-log-message-remove"),
			"emit": template("template-log-message-emit"),
			"invoke": template("template-log-message-invoke"),
			"invokeResult": template("template-log-message-invoke-result"),
		}
		this.elements = [];
		this.waitingMessages = [];
		this.enabled = true;
		this.startStopButtonIcon = document.querySelector("#log-start-stop i");
		this.startStopButtonText = document.querySelector("#log-start-stop span");
		this.colorer = new UuidColorer();
		
		system.addEventListener("event", ({ event, data }) => {
			if (event == "log") {
				this.addLogMessage(data);
			}
		});
		
		document.getElementById("log-start-stop").addEventListener("click", this.toggleStartStop.bind(this));
	}
	
	addLogMessage(message) {
		if (this.enabled) {
			let elem = this.templates[message.type].cloneNode(true);
			renderLogMessage(elem, message, this.colorer);
			
			this.elements.push(elem);
			this.container.append(elem);
			
			while (this.elements.length > MAX_LOG_MESSAGES) {
				this.elements.shift().remove();
			}
			
			this.scrollToBottom();
		} else {
			this.waitingMessages.push(message);
			this.waitingMessages.splice(0, this.waitingMessages.length - MAX_LOG_MESSAGES);
		}
	}
	
	start() {
		if (this.enabled) return;
		
		let removeCount = this.elements.length + this.waitingMessages.length - MAX_LOG_MESSAGES;
		for (let i = 0; i < removeCount; i++) {
			this.elements.shift().remove();
		}
		
		for (let message of this.waitingMessages) {
			let elem = this.templates[message.type].cloneNode(true);
			renderLogMessage(elem, message, this.colorer);
			
			this.elements.push(elem);
			this.container.append(elem);
		}
		
		this.waitingMessages = [];
		this.enabled = true;
		
		this.scrollToBottom();
		
		this.startStopButtonIcon.classList.remove("play");
		this.startStopButtonIcon.classList.add("pause");
		this.startStopButtonText.innerText = "stop";
	}
	
	stop() {
		this.enabled = false;
		
		this.startStopButtonIcon.classList.remove("pause");
		this.startStopButtonIcon.classList.add("play");
		this.startStopButtonText.innerText = "start";
	}
	
	toggleStartStop() {
		if (this.enabled) {
			this.stop();
		} else {
			this.start();
		}
	}
	
	scrollToBottom() {
		this.container.scrollTop = this.container.scrollHeight;
	}
}
