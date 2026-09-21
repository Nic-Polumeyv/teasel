function or() {
	"use strict"
	| 1;
	with (o) {}
}
function within() {
	"use strict"
	in o;
	with (o) {}
}
function instance() {
	"use strict"
	instanceof o;
	with (o) {}
}
