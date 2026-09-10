const f = <T>(x: T) => x;
const g = <T extends unknown>(x: T) => x;
const h = async <T>(x: T) => x;
const i = async <T>(x: T): Promise<T> => x;
