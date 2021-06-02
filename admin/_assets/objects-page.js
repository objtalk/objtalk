import inspect from "/_assets/vendor/browser-util-inspect/index.js";
import { template } from "./utils.js";

function renderObject(elem, object) {
	elem.querySelector(".anchor").id = "objects/" + object.name;
	
	elem.querySelector(".object-name").innerText = object.name;
	elem.querySelector(".object-name").href = "#objects/" + object.name;
	elem.querySelector(".object-value code").innerText = inspect(object.value);
	elem.querySelector(".object-last-modified").innerText = new Date(object.lastModified).toLocaleString();
	
	hljs.highlightElement(elem.querySelector(".object-value code"));
}

export default class ObjectsPage {
	constructor(conn) {
		this.container = document.getElementById("object-cards");
		this.template = template("template-object-card");
		this.elements = {};
		
		this.query = conn.query("*");
		
		this.query.addEventListener("open", objects => {
			objects.sort((a, b) => a.name.localeCompare(b.name));
			
			this.container.innerHTML = "";
			this.elements = {};

			for (let object of objects) {
				let elem = this.template.cloneNode(true);
				renderObject(elem, object);
				this.elements[object.name] = elem;
				this.container.append(elem);
			}
		});

		this.query.addEventListener("add", object => {
			let elem = this.template.cloneNode(true);
			renderObject(elem, object);
			this.elements[object.name] = elem;
			
			let objects = Object.values(this.query.objects);
			objects.sort((a, b) => a.name.localeCompare(b.name));
			
			let index = objects.findIndex(o => o.name == object.name);
			for (let i = index+1; objects[i]; i++) {
				if (this.elements[objects[i].name]) {
					this.container.insertBefore(elem, this.elements[objects[i].name]);
					return;
				}
			}
			
			this.container.append(elem);
		});

		this.query.addEventListener("change", object => {
			renderObject(this.elements[object.name], object);
		});

		this.query.addEventListener("remove", object => {
			this.elements[object.name].remove();
			delete this.elements[object.name];
		});
	}
}
