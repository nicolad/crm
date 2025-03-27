up: shuttle run

fe: npm run dev

build: npm run build

deploy: build
  shuttle deploy

deploy-ad: build
  shuttle deploy --ad

test: hurl hurl/register.hurl --verbose
