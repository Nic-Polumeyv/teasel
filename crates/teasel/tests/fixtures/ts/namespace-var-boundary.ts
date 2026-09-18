import D from "./d";
declare global {
	var D: number;
}
namespace N {
	var x = 1;
}
const x = 2;
declare module "buffer" {
	export class Blob {}
	global {
		var Blob: number;
	}
}
