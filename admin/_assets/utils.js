export function template(elementId) {
	let elem = document.createElement("div");
	elem.innerHTML = document.getElementById(elementId).innerHTML.trim();
	return elem.firstChild;
}
