function handler(event) {
	const request = event.request;
	const uri = request.uri;
	const page = uri.replace(/(\/index)?\.html$|\/$/, '') || '/';
	if (request.headers.host.value !== 'teasel.dev' || page !== uri) {
		return { statusCode: 301, statusDescription: 'Moved Permanently', headers: { location: { value: `https://teasel.dev${page}` } } };
	}
	if (uri.endsWith('/')) request.uri += 'index.html';
	else if (!uri.includes('.')) request.uri += '.html';
	return request;
}
