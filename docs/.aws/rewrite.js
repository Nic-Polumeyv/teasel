function handler(event) {
	const request = event.request;
	const uri = request.uri;
	// the reference the 0.0.15 package links to became the guide; the grammar wire's page went when the grammar crossed as JSON
	const page = (uri.replace(/(\/index)?\.html$|\/$/, '') || '/').replace(/^\/host-grammar$/, '/parsing-with-a-grammar').replace(/^\/reference\/wire$/, '/reference/grammar');
	if (request.headers.host.value !== 'teasel.dev' || page !== uri) {
		return { statusCode: 301, statusDescription: 'Moved Permanently', headers: { location: { value: `https://teasel.dev${page}` } } };
	}
	if (uri.endsWith('/')) request.uri += 'index.html';
	else if (!uri.includes('.')) request.uri += '.html';
	return request;
}
